//! Read-mostly queries the moderation commands need (`/getban`,
//! `/getidles`, `/restore`). Kept in their own file so they don't collide
//! with the rest of the storage layer; no tables of their own.

use crate::games::{GameId, Games};
use crate::players::{PlayerRecord, Players};
use crate::{bool_from_db, enum_to_db, DbResult};
use shared::KillMethod;
use sqlx::Row;

impl Players<'_> {
    /// Case-insensitive, without the `@` (Telegram usernames are).
    pub async fn by_username(&self, username: &str) -> DbResult<Option<PlayerRecord>> {
        let row = sqlx::query(
            "SELECT telegram_id, name, username, language, reachable, updated_at FROM players
             WHERE LOWER(username) = LOWER($1) ORDER BY updated_at DESC LIMIT 1",
        )
        .bind(username)
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
}

impl Games<'_> {
    /// When the player's first recorded game started (`/getban`'s "first
    /// seen").
    pub async fn first_started(&self, telegram_id: i64) -> DbResult<Option<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT MIN(g.started_at) FROM games g JOIN game_players gp ON gp.game_id = g.id WHERE gp.telegram_id = $1",
        )
        .bind(telegram_id)
        .fetch_one(self.store.pool())
        .await?)
    }

    /// How often the player was killed for idling since `since`, in any
    /// group or only in `group` (werewolf.sql `GetIdleKills24Hours` and
    /// `GetGroupIdleKills24Hours`).
    pub async fn idle_kills_since(&self, telegram_id: i64, since: i64, group: Option<i64>) -> DbResult<i64> {
        let idle = enum_to_db(&KillMethod::Idle);
        let count: i64 = match group {
            None => {
                sqlx::query_scalar("SELECT COUNT(*) FROM game_kills WHERE victim = $1 AND method = $2 AND at > $3")
                    .bind(telegram_id)
                    .bind(idle)
                    .bind(since)
                    .fetch_one(self.store.pool())
                    .await?
            }
            Some(group) => {
                sqlx::query_scalar(
                    "SELECT COUNT(*) FROM game_kills k JOIN games g ON g.id = k.game_id
                     WHERE k.victim = $1 AND k.method = $2 AND k.at > $3 AND g.group_id = $4",
                )
                .bind(telegram_id)
                .bind(idle)
                .bind(since)
                .bind(group)
                .fetch_one(self.store.pool())
                .await?
            }
        };
        Ok(count)
    }

    /// Every game the player took part in, oldest first, with its group.
    pub async fn games_of(&self, telegram_id: i64) -> DbResult<Vec<(GameId, i64)>> {
        let rows = sqlx::query(
            "SELECT g.id, g.group_id FROM games g JOIN game_players gp ON gp.game_id = g.id
             WHERE gp.telegram_id = $1 ORDER BY g.id",
        )
        .bind(telegram_id)
        .fetch_all(self.store.pool())
        .await?;
        rows.iter()
            .map(|r| Ok((GameId(r.try_get("id")?), r.try_get("group_id")?)))
            .collect()
    }

    /// `/restore` (werewolf.sql `RestoreAccount`): hands every game and
    /// kill of `old` to `new`. A game both accounts played in stays with
    /// `new` only. Returns how many games moved.
    pub async fn move_player_history(&self, old: i64, new: i64) -> DbResult<u64> {
        let mut tx = self.store.pool().begin().await?;
        let moved = sqlx::query(
            "UPDATE game_players SET telegram_id = $1 WHERE telegram_id = $2
             AND game_id NOT IN (SELECT game_id FROM game_players WHERE telegram_id = $3)",
        )
        .bind(new)
        .bind(old)
        .bind(new)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        sqlx::query("DELETE FROM game_players WHERE telegram_id = $1")
            .bind(old)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE game_kills SET victim = $1 WHERE victim = $2")
            .bind(new)
            .bind(old)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE game_kills SET killer = $1 WHERE killer = $2")
            .bind(new)
            .bind(old)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(moved)
    }
}
