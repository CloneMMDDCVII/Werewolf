//! Schema migrations, embedded in the binary and applied in order on
//! startup. Each migration carries one script per backend, because DDL is
//! the one place SQLite and Postgres differ (auto-increment keys); every
//! query outside this file is shared.
//!
//! Rules for adding one: append to `MIGRATIONS` with the next version,
//! never edit a released entry, and keep columns to `BIGINT`/`TEXT` so the
//! shared queries in the repositories decode the same on both backends.

use crate::{Backend, DbError, DbResult, Store};
use sqlx::Connection;
use std::path::Path;

pub(crate) struct Migration {
    pub version: i64,
    pub sqlite: &'static str,
    pub postgres: &'static str,
}

const V1_SQLITE: &str = r#"
CREATE TABLE chat_groups (
    telegram_id BIGINT PRIMARY KEY,
    name        TEXT   NOT NULL,
    settings    TEXT,
    updated_at  BIGINT NOT NULL
);
CREATE TABLE players (
    telegram_id BIGINT PRIMARY KEY,
    name        TEXT   NOT NULL,
    username    TEXT,
    language    TEXT,
    reachable   BIGINT NOT NULL DEFAULT 0,
    updated_at  BIGINT NOT NULL
);
CREATE TABLE bans (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    telegram_id     BIGINT NOT NULL,
    reason          TEXT   NOT NULL,
    issued_by_admin BIGINT,
    issued_by_rule  TEXT,
    issued_at       BIGINT NOT NULL,
    expires_at      BIGINT,
    lifted_at       BIGINT,
    lifted_by       BIGINT,
    source          TEXT   NOT NULL
);
CREATE INDEX bans_by_player ON bans (telegram_id);
CREATE TABLE games (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    group_id   BIGINT NOT NULL,
    group_name TEXT   NOT NULL,
    mode       TEXT   NOT NULL,
    started_at BIGINT NOT NULL,
    ended_at   BIGINT,
    result     TEXT
);
CREATE INDEX games_unfinished ON games (ended_at);
CREATE TABLE game_players (
    game_id     BIGINT NOT NULL REFERENCES games (id),
    telegram_id BIGINT NOT NULL,
    name        TEXT   NOT NULL,
    start_role  TEXT   NOT NULL,
    final_role  TEXT   NOT NULL,
    survived    BIGINT,
    won         BIGINT,
    PRIMARY KEY (game_id, telegram_id)
);
CREATE INDEX game_players_by_player ON game_players (telegram_id);
CREATE TABLE game_kills (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    game_id BIGINT NOT NULL REFERENCES games (id),
    victim  BIGINT NOT NULL,
    killer  BIGINT,
    method  TEXT   NOT NULL,
    day     BIGINT NOT NULL,
    at      BIGINT NOT NULL
);
CREATE INDEX game_kills_by_game ON game_kills (game_id);
"#;

const V1_POSTGRES: &str = r#"
CREATE TABLE chat_groups (
    telegram_id BIGINT PRIMARY KEY,
    name        TEXT   NOT NULL,
    settings    TEXT,
    updated_at  BIGINT NOT NULL
);
CREATE TABLE players (
    telegram_id BIGINT PRIMARY KEY,
    name        TEXT   NOT NULL,
    username    TEXT,
    language    TEXT,
    reachable   BIGINT NOT NULL DEFAULT 0,
    updated_at  BIGINT NOT NULL
);
CREATE TABLE bans (
    id              BIGSERIAL PRIMARY KEY,
    telegram_id     BIGINT NOT NULL,
    reason          TEXT   NOT NULL,
    issued_by_admin BIGINT,
    issued_by_rule  TEXT,
    issued_at       BIGINT NOT NULL,
    expires_at      BIGINT,
    lifted_at       BIGINT,
    lifted_by       BIGINT,
    source          TEXT   NOT NULL
);
CREATE INDEX bans_by_player ON bans (telegram_id);
CREATE TABLE games (
    id         BIGSERIAL PRIMARY KEY,
    group_id   BIGINT NOT NULL,
    group_name TEXT   NOT NULL,
    mode       TEXT   NOT NULL,
    started_at BIGINT NOT NULL,
    ended_at   BIGINT,
    result     TEXT
);
CREATE INDEX games_unfinished ON games (ended_at);
CREATE TABLE game_players (
    game_id     BIGINT NOT NULL REFERENCES games (id),
    telegram_id BIGINT NOT NULL,
    name        TEXT   NOT NULL,
    start_role  TEXT   NOT NULL,
    final_role  TEXT   NOT NULL,
    survived    BIGINT,
    won         BIGINT,
    PRIMARY KEY (game_id, telegram_id)
);
CREATE INDEX game_players_by_player ON game_players (telegram_id);
CREATE TABLE game_kills (
    id      BIGSERIAL PRIMARY KEY,
    game_id BIGINT NOT NULL REFERENCES games (id),
    victim  BIGINT NOT NULL,
    killer  BIGINT,
    method  TEXT   NOT NULL,
    day     BIGINT NOT NULL,
    at      BIGINT NOT NULL
);
CREATE INDEX game_kills_by_game ON game_kills (game_id);
"#;

pub(crate) const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sqlite: V1_SQLITE,
    postgres: V1_POSTGRES,
}];

/// The schema version this build of the bot expects.
pub const LATEST_SCHEMA_VERSION: i64 = 1;

/// Arbitrary but fixed: every bot process sharing a Postgres database
/// takes this advisory lock while migrating, so two bots starting at once
/// never apply the same migration twice.
const PG_MIGRATION_LOCK: i64 = 0x5745_5245_574f_4c46; // "WEREWOLF"

pub(crate) async fn run(store: &Store, sqlite_file: Option<&Path>) -> DbResult<()> {
    run_list(store, sqlite_file, MIGRATIONS).await
}

pub(crate) async fn run_list(store: &Store, sqlite_file: Option<&Path>, migrations: &[Migration]) -> DbResult<()> {
    let latest = migrations.last().map(|m| m.version).unwrap_or(0);
    let mut conn = store.pool().acquire().await?;

    if store.backend() == Backend::Postgres {
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(PG_MIGRATION_LOCK)
            .execute(&mut *conn)
            .await?;
    }

    let result = async {
        sqlx::query("CREATE TABLE IF NOT EXISTS schema_version (version BIGINT NOT NULL)")
            .execute(&mut *conn)
            .await?;
        let current: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM schema_version")
            .fetch_one(&mut *conn)
            .await?;

        if current > latest {
            return Err(DbError::NewerSchema {
                database: current,
                bot: latest,
            });
        }
        if current == latest {
            return Ok(());
        }
        if current > 0 {
            if let Some(file) = sqlite_file {
                backup_sqlite(&mut conn, file, current).await?;
            }
        }

        for m in migrations.iter().filter(|m| m.version > current) {
            let script = match store.backend() {
                Backend::Sqlite => m.sqlite,
                Backend::Postgres => m.postgres,
            };
            let mut tx = conn.begin().await?;
            for statement in split_statements(script) {
                sqlx::query(statement).execute(&mut *tx).await?;
            }
            sqlx::query("DELETE FROM schema_version").execute(&mut *tx).await?;
            sqlx::query("INSERT INTO schema_version (version) VALUES ($1)")
                .bind(m.version)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
        }
        Ok(())
    }
    .await;

    if store.backend() == Backend::Postgres {
        sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(PG_MIGRATION_LOCK)
            .execute(&mut *conn)
            .await?;
    }
    result
}

pub(crate) async fn current_version(store: &Store) -> DbResult<i64> {
    Ok(
        sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM schema_version")
            .fetch_one(store.pool())
            .await?,
    )
}

/// `VACUUM INTO` writes a consistent copy even with WAL readers attached,
/// unlike copying the file on disk.
async fn backup_sqlite(conn: &mut sqlx::AnyConnection, file: &Path, version: i64) -> DbResult<()> {
    let mut backup = file.as_os_str().to_owned();
    backup.push(format!(".bak-{version}"));
    let backup = std::path::PathBuf::from(backup);
    if backup.exists() {
        std::fs::remove_file(&backup).map_err(|e| DbError::Backup(e.to_string()))?;
    }
    let target = backup.to_string_lossy().replace('\'', "''");
    // The path is ours (the configured file plus a suffix), quotes escaped.
    sqlx::query(sqlx::AssertSqlSafe(format!("VACUUM INTO '{target}'")))
        .execute(&mut *conn)
        .await
        .map_err(|e| DbError::Backup(e.to_string()))?;
    Ok(())
}

/// The scripts above contain no semicolons inside literals, so a plain
/// split is enough; executing one statement at a time keeps both drivers
/// happy inside a transaction.
fn split_statements(script: &str) -> impl Iterator<Item = &str> {
    script.split(';').map(str::trim).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);

    fn temp_db() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "werewolf-db-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("werewolf.db")
    }

    const OLD: &[Migration] = &[Migration {
        version: 1,
        sqlite: "CREATE TABLE a (x BIGINT)",
        postgres: "CREATE TABLE a (x BIGINT)",
    }];
    const NEW: &[Migration] = &[
        Migration {
            version: 1,
            sqlite: "CREATE TABLE a (x BIGINT)",
            postgres: "CREATE TABLE a (x BIGINT)",
        },
        Migration {
            version: 2,
            sqlite: "CREATE TABLE b (y BIGINT)",
            postgres: "CREATE TABLE b (y BIGINT)",
        },
    ];

    async fn open_raw(file: &Path) -> Store {
        sqlx::any::install_default_drivers();
        let url = format!("sqlite://{}?mode=rwc", file.display());
        let pool = sqlx::any::AnyPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        Store {
            pool,
            backend: Backend::Sqlite,
        }
    }

    #[tokio::test]
    async fn the_database_file_is_created_with_the_current_schema() {
        let file = temp_db();
        assert!(!file.exists());
        let store = Store::connect(&format!("sqlite://{}", file.display())).await.unwrap();
        assert!(file.exists());
        assert_eq!(store.schema_version().await.unwrap(), LATEST_SCHEMA_VERSION);
    }

    #[tokio::test]
    async fn a_newer_bot_backs_up_then_migrates_an_older_database() {
        let file = temp_db();
        let store = open_raw(&file).await;
        run_list(&store, Some(&file), OLD).await.unwrap();
        sqlx::query("INSERT INTO a (x) VALUES (42)")
            .execute(store.pool())
            .await
            .unwrap();
        store.close().await;

        let store = open_raw(&file).await;
        run_list(&store, Some(&file), NEW).await.unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 2);
        sqlx::query("SELECT y FROM b").fetch_all(store.pool()).await.unwrap();

        let backup = PathBuf::from(format!("{}.bak-1", file.display()));
        assert!(backup.exists(), "expected a backup at {}", backup.display());
        let old = open_raw(&backup).await;
        let x: i64 = sqlx::query_scalar("SELECT x FROM a")
            .fetch_one(old.pool())
            .await
            .unwrap();
        assert_eq!(x, 42);
        assert_eq!(old.schema_version().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn a_fresh_database_is_not_backed_up() {
        let file = temp_db();
        let store = open_raw(&file).await;
        run_list(&store, Some(&file), NEW).await.unwrap();
        assert!(!PathBuf::from(format!("{}.bak-0", file.display())).exists());
    }

    #[tokio::test]
    async fn an_older_bot_refuses_a_newer_database() {
        let file = temp_db();
        let store = open_raw(&file).await;
        run_list(&store, Some(&file), NEW).await.unwrap();
        match run_list(&store, Some(&file), OLD).await {
            Err(DbError::NewerSchema { database: 2, bot: 1 }) => {}
            other => panic!("expected NewerSchema, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn migrating_twice_is_a_no_op() {
        let file = temp_db();
        let store = open_raw(&file).await;
        run_list(&store, Some(&file), NEW).await.unwrap();
        run_list(&store, Some(&file), NEW).await.unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 2);
    }
}
