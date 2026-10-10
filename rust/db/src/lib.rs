//! Persistent storage for the bot: one [`Store`] with a repository per
//! area ([`Groups`], [`Players`], [`Bans`], [`Games`]). Other crates never
//! write SQL themselves; they persist through these repositories.
//!
//! Two backends behind the same API, picked by the database URL:
//!
//! - `sqlite://werewolf.db` — the default. SQLite is compiled into the
//!   binary (`sqlite-bundled`), so there is nothing to install. For
//!   self-hosters, development and tests.
//! - `postgres://...` — for the main bot at legacy scale.
//!
//! Both go through `sqlx`'s `Any` driver, so every query is written once.
//! That keeps the SQL deliberately plain: `BIGINT` and `TEXT` columns only,
//! numbered `$1` placeholders (which both backends accept), timestamps as
//! Unix seconds passed in by the caller (so tests control time), and
//! booleans as `0`/`1`. Schema DDL is the one place the dialects differ;
//! see `migrations`.
//!
//! Replaces the earlier plan (PostgreSQL only, migrated from the SQL Server
//! schema in `werewolf.sql`): importing the legacy data is still to do, and
//! lands in these same tables.

mod bans;
mod games;
mod groups;
mod migrations;
mod moderation_queries;
mod players;

pub use bans::{BanIssuer, BanRecord, BanSource, Bans, NewBan};
pub use games::{
    FixtureGame, FixtureKill, FixturePlayer, GameId, GameResult, Games, NewGame, NewGamePlayer, UnfinishedGame,
};
pub use groups::{GroupRecord, Groups};
pub use migrations::LATEST_SCHEMA_VERSION;
pub use players::{PlayerRecord, Players};

use sqlx::any::{AnyConnectOptions, AnyPoolOptions};
use sqlx::{AnyPool, Executor};
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Sqlite,
    Postgres,
}

#[derive(Debug)]
pub enum DbError {
    /// The URL is neither `sqlite:` nor `postgres:`/`postgresql:`.
    UnsupportedUrl(String),
    /// The database was migrated by a newer bot than this one. Starting
    /// anyway would run old code against a schema it doesn't know.
    NewerSchema {
        database: i64,
        bot: i64,
    },
    /// A stored value didn't parse back (e.g. a role name this build has
    /// no variant for, or settings JSON that is not JSON).
    Corrupt(String),
    /// Copying the SQLite file before a migration failed; the migration
    /// was not attempted.
    Backup(String),
    Sql(sqlx::Error),
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::UnsupportedUrl(url) => write!(
                f,
                "unsupported database URL {url:?}: use sqlite://path/to/file.db or postgres://user@host/db"
            ),
            DbError::NewerSchema { database, bot } => write!(
                f,
                "the database is at schema version {database}, newer than this bot's {bot}; \
                 run a newer werewolf-bot or point it at another database"
            ),
            DbError::Corrupt(msg) => write!(f, "unreadable value in the database: {msg}"),
            DbError::Backup(msg) => write!(f, "could not back up the database before migrating: {msg}"),
            DbError::Sql(e) => write!(f, "database error: {e}"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<sqlx::Error> for DbError {
    fn from(e: sqlx::Error) -> Self {
        DbError::Sql(e)
    }
}

pub type DbResult<T> = Result<T, DbError>;

#[derive(Clone)]
pub struct Store {
    pool: AnyPool,
    backend: Backend,
}

impl Store {
    /// Opens (and for SQLite, creates) the database at `url`, then brings
    /// its schema up to date. A SQLite file that already holds data is
    /// copied to `<file>.bak-<old version>` before any migration runs.
    pub async fn connect(url: &str) -> DbResult<Self> {
        sqlx::any::install_default_drivers();
        let backend = backend_of(url)?;
        let (url, sqlite_file) = match backend {
            Backend::Sqlite => normalize_sqlite_url(url),
            Backend::Postgres => (url.to_string(), None),
        };
        let options = AnyConnectOptions::from_str(&url)?;
        let pool = pool_options(backend, sqlite_file.is_none())
            .connect_with(options)
            .await?;
        let store = Store { pool, backend };
        migrations::run(&store, sqlite_file.as_deref()).await?;
        Ok(store)
    }

    /// A private, empty in-memory SQLite database with the current schema.
    /// For tests: the exact same code as the file-backed store.
    pub async fn in_memory() -> DbResult<Self> {
        Self::connect("sqlite::memory:").await
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }

    pub fn groups(&self) -> Groups<'_> {
        Groups { store: self }
    }

    pub fn players(&self) -> Players<'_> {
        Players { store: self }
    }

    pub fn bans(&self) -> Bans<'_> {
        Bans { store: self }
    }

    pub fn games(&self) -> Games<'_> {
        Games { store: self }
    }

    /// The schema version currently applied.
    pub async fn schema_version(&self) -> DbResult<i64> {
        migrations::current_version(self).await
    }

    /// One trivial round trip (`SELECT 1`), for health checks. Waits for a
    /// free connection like any query, so callers wanting a bound wrap it
    /// in a timeout.
    pub async fn ping(&self) -> DbResult<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub(crate) fn pool(&self) -> &AnyPool {
        &self.pool
    }
}

fn backend_of(url: &str) -> DbResult<Backend> {
    if url.starts_with("sqlite:") {
        Ok(Backend::Sqlite)
    } else if url.starts_with("postgres:") || url.starts_with("postgresql:") {
        Ok(Backend::Postgres)
    } else {
        Err(DbError::UnsupportedUrl(url.to_string()))
    }
}

/// Adds `mode=rwc` so a missing SQLite file is created rather than being
/// an error, and returns the file path (None for in-memory databases).
fn normalize_sqlite_url(url: &str) -> (String, Option<PathBuf>) {
    let rest = url.trim_start_matches("sqlite:").trim_start_matches("//");
    let (path, query) = match rest.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (rest, None),
    };
    if path.is_empty() || path == ":memory:" {
        return (url.to_string(), None);
    }
    let query = match query {
        Some(q) if q.contains("mode=") => q.to_string(),
        Some(q) => format!("{q}&mode=rwc"),
        None => "mode=rwc".to_string(),
    };
    (format!("sqlite://{path}?{query}"), Some(PathBuf::from(path)))
}

fn pool_options(backend: Backend, in_memory: bool) -> AnyPoolOptions {
    match backend {
        // Every connection to `:memory:` is its own empty database, so an
        // in-memory store must stay on one connection for its lifetime.
        Backend::Sqlite if in_memory => AnyPoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None),
        // WAL lets readers run alongside the single writer; busy_timeout
        // makes a second writer wait instead of failing with SQLITE_BUSY.
        Backend::Sqlite => AnyPoolOptions::new().max_connections(4).after_connect(|conn, _| {
            Box::pin(async move {
                conn.execute("PRAGMA journal_mode = WAL").await?;
                conn.execute("PRAGMA busy_timeout = 5000").await?;
                conn.execute("PRAGMA foreign_keys = ON").await?;
                Ok(())
            })
        }),
        Backend::Postgres => AnyPoolOptions::new().max_connections(10),
    }
}

pub(crate) fn bool_to_db(b: bool) -> i64 {
    b as i64
}

pub(crate) fn bool_from_db(v: i64) -> bool {
    v != 0
}

/// Stores enums by their `serde` name — the same names the legacy
/// database and the fixtures use (`"Seer"`, `"Lynch"`, ...).
pub(crate) fn enum_to_db<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        other => panic!("enum_to_db used on a non-unit enum: {other:?}"),
    }
}

pub(crate) fn enum_from_db<T: serde::de::DeserializeOwned>(raw: &str) -> DbResult<T> {
    serde_json::from_value(serde_json::Value::String(raw.to_string()))
        .map_err(|_| DbError::Corrupt(format!("unknown value {raw:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_urls_get_create_mode() {
        assert_eq!(
            normalize_sqlite_url("sqlite://werewolf.db"),
            (
                "sqlite://werewolf.db?mode=rwc".to_string(),
                Some(PathBuf::from("werewolf.db"))
            )
        );
        assert_eq!(
            normalize_sqlite_url("sqlite:///srv/wolf/werewolf.db"),
            (
                "sqlite:///srv/wolf/werewolf.db?mode=rwc".to_string(),
                Some(PathBuf::from("/srv/wolf/werewolf.db"))
            )
        );
        assert_eq!(normalize_sqlite_url("sqlite::memory:").1, None);
    }

    #[test]
    fn unknown_schemes_are_rejected() {
        assert!(matches!(backend_of("mysql://x"), Err(DbError::UnsupportedUrl(_))));
        assert_eq!(backend_of("postgresql://x").unwrap(), Backend::Postgres);
    }
}
