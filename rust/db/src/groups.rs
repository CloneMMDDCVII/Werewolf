//! Telegram groups the bot has seen, and each group's settings.
//!
//! Settings are stored as one JSON document per group rather than a column
//! per setting, so the group-config code can add a setting without a
//! migration. The settings type is the caller's: give it
//! `#[serde(default)]` at the struct level and a `Default` impl with the
//! legacy defaults, and a stored document missing a newer field reads back
//! with that field's default while every stored value is kept.

use crate::{DbError, DbResult, Store};
use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx::Row;

#[derive(Debug, Clone, PartialEq)]
pub struct GroupRecord {
    pub telegram_id: i64,
    pub name: String,
    /// `None` until a setting is first changed.
    pub settings: Option<serde_json::Value>,
    pub updated_at: i64,
}

pub struct Groups<'a> {
    pub(crate) store: &'a Store,
}

impl Groups<'_> {
    pub async fn get(&self, telegram_id: i64) -> DbResult<Option<GroupRecord>> {
        let row = sqlx::query("SELECT telegram_id, name, settings, updated_at FROM chat_groups WHERE telegram_id = $1")
            .bind(telegram_id)
            .fetch_optional(self.store.pool())
            .await?;
        row.map(|r| {
            let settings: Option<String> = r.try_get("settings")?;
            Ok(GroupRecord {
                telegram_id: r.try_get("telegram_id")?,
                name: r.try_get("name")?,
                settings: settings.map(|s| parse_json(&s)).transpose()?,
                updated_at: r.try_get("updated_at")?,
            })
        })
        .transpose()
    }

    /// Records the group's current name (Telegram groups get renamed),
    /// creating the row if this is the first time the bot sees the group.
    /// Settings are left untouched.
    pub async fn upsert_name(&self, telegram_id: i64, name: &str, now: i64) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO chat_groups (telegram_id, name, settings, updated_at) VALUES ($1, $2, NULL, $3)
             ON CONFLICT (telegram_id) DO UPDATE SET name = excluded.name, updated_at = excluded.updated_at",
        )
        .bind(telegram_id)
        .bind(name)
        .bind(now)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    /// The group's settings, or `T::default()` for a group with none
    /// stored. Reading never writes.
    pub async fn settings<T: DeserializeOwned + Default>(&self, telegram_id: i64) -> DbResult<T> {
        match self.get(telegram_id).await?.and_then(|g| g.settings) {
            Some(json) => serde_json::from_value(json).map_err(|e| DbError::Corrupt(e.to_string())),
            None => Ok(T::default()),
        }
    }

    /// Replaces the group's settings document. `name` is only used when the
    /// group has no row yet.
    pub async fn put_settings<T: Serialize>(
        &self,
        telegram_id: i64,
        name: &str,
        settings: &T,
        now: i64,
    ) -> DbResult<()> {
        let json = serde_json::to_string(settings).map_err(|e| DbError::Corrupt(e.to_string()))?;
        sqlx::query(
            "INSERT INTO chat_groups (telegram_id, name, settings, updated_at) VALUES ($1, $2, $3, $4)
             ON CONFLICT (telegram_id) DO UPDATE SET settings = excluded.settings, updated_at = excluded.updated_at",
        )
        .bind(telegram_id)
        .bind(name)
        .bind(json)
        .bind(now)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }
}

fn parse_json(raw: &str) -> Result<serde_json::Value, sqlx::Error> {
    serde_json::from_str(raw).map_err(|e| sqlx::Error::Decode(Box::new(e)))
}
