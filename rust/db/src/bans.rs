//! Ban history. Rows are never deleted: lifting a ban or letting it expire
//! keeps it in the player's history. Mirrors `moderation::Ban`'s shape,
//! with timestamps as Unix seconds instead of strings.

use crate::{enum_from_db, enum_to_db, DbResult, Store};
use serde::{Deserialize, Serialize};
use sqlx::any::AnyRow;
use sqlx::Row;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BanIssuer {
    Admin { telegram_id: i64 },
    System { rule: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BanSource {
    /// Carried over from the legacy `GlobalBan` table.
    Migrated,
    Native,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBan {
    pub telegram_id: i64,
    pub reason: String,
    pub issued_by: BanIssuer,
    pub issued_at: i64,
    /// `None` is permanent.
    pub expires_at: Option<i64>,
    pub source: BanSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BanRecord {
    pub id: i64,
    pub telegram_id: i64,
    pub reason: String,
    pub issued_by: BanIssuer,
    pub issued_at: i64,
    pub expires_at: Option<i64>,
    pub lifted_at: Option<i64>,
    pub lifted_by: Option<i64>,
    pub source: BanSource,
}

impl BanRecord {
    pub fn is_active(&self, now: i64) -> bool {
        self.lifted_at.is_none() && self.expires_at.is_none_or(|t| t > now)
    }
}

pub struct Bans<'a> {
    pub(crate) store: &'a Store,
}

/// Queries are spelled out in full (no `format!`): sqlx only accepts
/// `&'static str` SQL, which keeps every query auditable as written.
macro_rules! select_bans {
    ($where:literal) => {
        concat!(
            "SELECT id, telegram_id, reason, issued_by_admin, issued_by_rule, issued_at, expires_at, ",
            "lifted_at, lifted_by, source FROM bans WHERE ",
            $where
        )
    };
}

impl Bans<'_> {
    pub async fn insert(&self, ban: &NewBan) -> DbResult<i64> {
        let (admin, rule) = match &ban.issued_by {
            BanIssuer::Admin { telegram_id } => (Some(*telegram_id), None),
            BanIssuer::System { rule } => (None, Some(rule.as_str())),
        };
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO bans (telegram_id, reason, issued_by_admin, issued_by_rule, issued_at, expires_at, source)
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
        )
        .bind(ban.telegram_id)
        .bind(&ban.reason)
        .bind(admin)
        .bind(rule)
        .bind(ban.issued_at)
        .bind(ban.expires_at)
        .bind(enum_to_db(&ban.source))
        .fetch_one(self.store.pool())
        .await?;
        Ok(id)
    }

    /// The ban currently in force for this player, if any. With several
    /// overlapping bans, the one that lasts longest (permanent first).
    pub async fn active(&self, telegram_id: i64, now: i64) -> DbResult<Option<BanRecord>> {
        let rows = sqlx::query(select_bans!(
            "telegram_id = $2 AND lifted_at IS NULL AND (expires_at IS NULL OR expires_at > $1)"
        ))
        .bind(now)
        .bind(telegram_id)
        .fetch_all(self.store.pool())
        .await?;
        let mut bans = rows.iter().map(from_row).collect::<DbResult<Vec<_>>>()?;
        bans.sort_by_key(|b| std::cmp::Reverse(b.expires_at.unwrap_or(i64::MAX)));
        Ok(bans.into_iter().next())
    }

    pub async fn is_banned(&self, telegram_id: i64, now: i64) -> DbResult<bool> {
        Ok(self.active(telegram_id, now).await?.is_some())
    }

    /// Every player with a ban in force, for loading into memory at startup.
    pub async fn all_active(&self, now: i64) -> DbResult<Vec<BanRecord>> {
        let rows = sqlx::query(select_bans!(
            "lifted_at IS NULL AND (expires_at IS NULL OR expires_at > $1) ORDER BY id"
        ))
            .bind(now)
            .fetch_all(self.store.pool())
            .await?;
        rows.iter().map(from_row).collect()
    }

    /// Every ban the player ever had, oldest first, lifted and expired
    /// ones included.
    pub async fn history(&self, telegram_id: i64) -> DbResult<Vec<BanRecord>> {
        let rows = sqlx::query(select_bans!("telegram_id = $1 ORDER BY id"))
            .bind(telegram_id)
            .fetch_all(self.store.pool())
            .await?;
        rows.iter().map(from_row).collect()
    }

    /// Lifts every ban in force for this player; returns how many.
    pub async fn lift(&self, telegram_id: i64, lifted_by: i64, now: i64) -> DbResult<u64> {
        let done = sqlx::query(
            "UPDATE bans SET lifted_at = $1, lifted_by = $2
             WHERE telegram_id = $3 AND lifted_at IS NULL AND (expires_at IS NULL OR expires_at > $4)",
        )
        .bind(now)
        .bind(lifted_by)
        .bind(telegram_id)
        .bind(now)
        .execute(self.store.pool())
        .await?;
        Ok(done.rows_affected())
    }
}

fn from_row(r: &AnyRow) -> DbResult<BanRecord> {
    let admin: Option<i64> = r.try_get("issued_by_admin")?;
    let rule: Option<String> = r.try_get("issued_by_rule")?;
    let issued_by = match (admin, rule) {
        (Some(telegram_id), _) => BanIssuer::Admin { telegram_id },
        (None, Some(rule)) => BanIssuer::System { rule },
        (None, None) => BanIssuer::System { rule: String::new() },
    };
    let source: String = r.try_get("source")?;
    Ok(BanRecord {
        id: r.try_get("id")?,
        telegram_id: r.try_get("telegram_id")?,
        reason: r.try_get("reason")?,
        issued_by,
        issued_at: r.try_get("issued_at")?,
        expires_at: r.try_get("expires_at")?,
        lifted_at: r.try_get("lifted_at")?,
        lifted_by: r.try_get("lifted_by")?,
        source: enum_from_db(&source)?,
    })
}
