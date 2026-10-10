//! Telegram users the bot knows about. A player row exists once someone
//! has started the bot privately or joined a game; `reachable` says
//! whether the bot can send them private messages (night actions need it).

use crate::{bool_from_db, bool_to_db, DbResult, Store};
use sqlx::Row;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRecord {
    pub telegram_id: i64,
    pub name: String,
    pub username: Option<String>,
    /// Language file name (e.g. "English"); `None` means "use the group's".
    pub language: Option<String>,
    pub reachable: bool,
    pub updated_at: i64,
}

pub struct Players<'a> {
    pub(crate) store: &'a Store,
}

impl Players<'_> {
    pub async fn get(&self, telegram_id: i64) -> DbResult<Option<PlayerRecord>> {
        let row = sqlx::query(
            "SELECT telegram_id, name, username, language, reachable, updated_at FROM players WHERE telegram_id = $1",
        )
        .bind(telegram_id)
        .fetch_optional(self.store.pool())
        .await?;
        Ok(row
            .map(|r| -> Result<_, sqlx::Error> {
                Ok(PlayerRecord {
                    telegram_id: r.try_get("telegram_id")?,
                    name: r.try_get("name")?,
                    username: r.try_get("username")?,
                    language: r.try_get("language")?,
                    reachable: bool_from_db(r.try_get("reachable")?),
                    updated_at: r.try_get("updated_at")?,
                })
            })
            .transpose()?)
    }

    /// Creates the player or refreshes their name and username, keeping
    /// their language and reachability.
    pub async fn upsert(&self, telegram_id: i64, name: &str, username: Option<&str>, now: i64) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO players (telegram_id, name, username, language, reachable, updated_at)
             VALUES ($1, $2, $3, NULL, 0, $4)
             ON CONFLICT (telegram_id) DO UPDATE
             SET name = excluded.name, username = excluded.username, updated_at = excluded.updated_at",
        )
        .bind(telegram_id)
        .bind(name)
        .bind(username)
        .bind(now)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    /// Set when the player starts the bot privately; cleared when Telegram
    /// says they blocked it.
    pub async fn set_reachable(&self, telegram_id: i64, reachable: bool, now: i64) -> DbResult<()> {
        sqlx::query("UPDATE players SET reachable = $1, updated_at = $2 WHERE telegram_id = $3")
            .bind(bool_to_db(reachable))
            .bind(now)
            .bind(telegram_id)
            .execute(self.store.pool())
            .await?;
        Ok(())
    }

    pub async fn set_language(&self, telegram_id: i64, language: Option<&str>, now: i64) -> DbResult<()> {
        sqlx::query("UPDATE players SET language = $1, updated_at = $2 WHERE telegram_id = $3")
            .bind(language)
            .bind(now)
            .bind(telegram_id)
            .execute(self.store.pool())
            .await?;
        Ok(())
    }

    /// Whether the player appears in any recorded game (`/getban` answers
    /// differently for someone who never played).
    pub async fn has_played(&self, telegram_id: i64) -> DbResult<bool> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM game_players WHERE telegram_id = $1")
            .bind(telegram_id)
            .fetch_one(self.store.pool())
            .await?;
        Ok(n > 0)
    }
}
