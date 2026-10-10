//! Game history: one row per game, its players and its deaths. Same three
//! tables as the legacy `Game`, `GamePlayer` and `GameKill`, so a recorded
//! game exports to the fixture format `sim` replays.
//!
//! A game row is written when the game starts and closed when it ends. A
//! row still open when the bot starts belongs to a game the last process
//! never finished (crash or forced exit): see [`Games::unfinished`].

use crate::{bool_from_db, bool_to_db, enum_from_db, enum_to_db, DbError, DbResult, Store};
use serde::{Deserialize, Serialize};
use shared::{GameMode, KillMethod, Role, Team};
use sqlx::Row;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GameId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewGamePlayer {
    pub telegram_id: i64,
    pub name: String,
    pub role: Role,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewGame {
    pub group_id: i64,
    pub group_name: String,
    pub mode: GameMode,
    pub started_at: i64,
    pub players: Vec<NewGamePlayer>,
}

/// How a game ended. Stored as text: a team's name for a win (the legacy
/// winner strings), or one of the non-win endings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameResult {
    Won(Team),
    /// `/killgame` or a forced shutdown.
    Killed,
    /// The process stopped without ending the game.
    Interrupted,
    /// The engine could not determine a winner (`WinOutcome`'s
    /// `RngDependent`/`InsufficientData`/`Unimplemented`), with its reason.
    Unresolved(String),
}

impl GameResult {
    fn to_db(&self) -> String {
        match self {
            GameResult::Won(team) => enum_to_db(team),
            GameResult::Killed => "#Killed".into(),
            GameResult::Interrupted => "#Interrupted".into(),
            GameResult::Unresolved(why) => format!("#Unresolved:{why}"),
        }
    }

    fn from_db(raw: &str) -> DbResult<Self> {
        Ok(match raw {
            "#Killed" => GameResult::Killed,
            "#Interrupted" => GameResult::Interrupted,
            _ => match raw.strip_prefix("#Unresolved:") {
                Some(why) => GameResult::Unresolved(why.to_string()),
                None => GameResult::Won(enum_from_db(raw)?),
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnfinishedGame {
    pub id: GameId,
    pub group_id: i64,
    pub group_name: String,
    pub started_at: i64,
}

/// One exported game, field for field the shape of `sim`'s fixture files
/// (`sim/fixtures/*.json`), so a recorded game can be replayed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureGame {
    pub game_id: i64,
    pub group_name: String,
    pub group_telegram_id: i64,
    pub time_started: String,
    pub time_ended: String,
    pub winner: String,
    pub game_mode: String,
    pub players: Vec<FixturePlayer>,
    pub kills: Vec<FixtureKill>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixturePlayer {
    pub telegram_id: i64,
    pub name: String,
    pub role: String,
    pub survived: bool,
    pub won: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureKill {
    /// 0 when the killer is unknown: the engine reports who died and how,
    /// not always who did it.
    pub killer_telegram_id: i64,
    pub victim_telegram_id: i64,
    pub killer_name: String,
    pub victim_name: String,
    pub kill_method: String,
    pub day: u32,
    pub timestamp: String,
}

pub struct Games<'a> {
    pub(crate) store: &'a Store,
}

impl Games<'_> {
    pub async fn start(&self, game: &NewGame) -> DbResult<GameId> {
        let mut tx = self.store.pool().begin().await?;
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO games (group_id, group_name, mode, started_at) VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(game.group_id)
        .bind(&game.group_name)
        .bind(enum_to_db(&game.mode))
        .bind(game.started_at)
        .fetch_one(&mut *tx)
        .await?;
        for p in &game.players {
            let role = enum_to_db(&p.role);
            sqlx::query(
                "INSERT INTO game_players (game_id, telegram_id, name, start_role, final_role) VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(id)
            .bind(p.telegram_id)
            .bind(&p.name)
            .bind(&role)
            .bind(&role)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(GameId(id))
    }

    pub async fn record_kill(
        &self,
        game: GameId,
        victim: i64,
        killer: Option<i64>,
        method: KillMethod,
        day: u32,
        at: i64,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO game_kills (game_id, victim, killer, method, day, at) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(game.0)
        .bind(victim)
        .bind(killer)
        .bind(enum_to_db(&method))
        .bind(day as i64)
        .bind(at)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    /// A transform changed this player's role (Traitor to Wolf, ...).
    pub async fn set_final_role(&self, game: GameId, telegram_id: i64, role: Role) -> DbResult<()> {
        sqlx::query("UPDATE game_players SET final_role = $1 WHERE game_id = $2 AND telegram_id = $3")
            .bind(enum_to_db(&role))
            .bind(game.0)
            .bind(telegram_id)
            .execute(self.store.pool())
            .await?;
        Ok(())
    }

    /// Closes the game. `players` gives each player's `(telegram_id,
    /// survived, won)`; anyone not listed keeps unknown survival.
    pub async fn finish(
        &self,
        game: GameId,
        result: &GameResult,
        ended_at: i64,
        players: &[(i64, bool, bool)],
    ) -> DbResult<()> {
        let mut tx = self.store.pool().begin().await?;
        sqlx::query("UPDATE games SET result = $1, ended_at = $2 WHERE id = $3")
            .bind(result.to_db())
            .bind(ended_at)
            .bind(game.0)
            .execute(&mut *tx)
            .await?;
        for &(telegram_id, survived, won) in players {
            sqlx::query("UPDATE game_players SET survived = $1, won = $2 WHERE game_id = $3 AND telegram_id = $4")
                .bind(bool_to_db(survived))
                .bind(bool_to_db(won))
                .bind(game.0)
                .bind(telegram_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Games that were started but never finished, oldest first.
    pub async fn unfinished(&self) -> DbResult<Vec<UnfinishedGame>> {
        let rows =
            sqlx::query("SELECT id, group_id, group_name, started_at FROM games WHERE ended_at IS NULL ORDER BY id")
                .fetch_all(self.store.pool())
                .await?;
        rows.iter()
            .map(|r| {
                Ok(UnfinishedGame {
                    id: GameId(r.try_get("id")?),
                    group_id: r.try_get("group_id")?,
                    group_name: r.try_get("group_name")?,
                    started_at: r.try_get("started_at")?,
                })
            })
            .collect()
    }

    pub async fn mark_interrupted(&self, game: GameId, at: i64) -> DbResult<()> {
        self.finish(game, &GameResult::Interrupted, at, &[]).await
    }

    pub async fn result(&self, game: GameId) -> DbResult<Option<GameResult>> {
        let raw: Option<String> = sqlx::query_scalar("SELECT result FROM games WHERE id = $1")
            .bind(game.0)
            .fetch_one(self.store.pool())
            .await?;
        raw.map(|r| GameResult::from_db(&r)).transpose()
    }

    /// Exports a finished game in the fixture shape. Only games that ended
    /// with a winner can be exported: the fixture format has no field for
    /// "killed" or "interrupted".
    pub async fn export_fixture(&self, game: GameId) -> DbResult<FixtureGame> {
        let g =
            sqlx::query("SELECT id, group_id, group_name, mode, started_at, ended_at, result FROM games WHERE id = $1")
                .bind(game.0)
                .fetch_one(self.store.pool())
                .await?;
        let result: Option<String> = g.try_get("result")?;
        let winner = match result.as_deref().map(GameResult::from_db).transpose()? {
            Some(GameResult::Won(team)) => team,
            other => {
                return Err(DbError::Corrupt(format!(
                    "game {} has no winner to export (result: {other:?})",
                    game.0
                )))
            }
        };
        let ended_at: Option<i64> = g.try_get("ended_at")?;

        let players = sqlx::query(
            "SELECT telegram_id, name, final_role, survived, won FROM game_players WHERE game_id = $1 ORDER BY telegram_id",
        )
        .bind(game.0)
        .fetch_all(self.store.pool())
        .await?;
        let mut names = std::collections::HashMap::new();
        let players = players
            .iter()
            .map(|r| {
                let telegram_id: i64 = r.try_get("telegram_id")?;
                let name: String = r.try_get("name")?;
                names.insert(telegram_id, name.clone());
                Ok(FixturePlayer {
                    telegram_id,
                    name,
                    role: r.try_get("final_role")?,
                    survived: bool_from_db(r.try_get::<Option<i64>, _>("survived")?.unwrap_or(0)),
                    won: bool_from_db(r.try_get::<Option<i64>, _>("won")?.unwrap_or(0)),
                })
            })
            .collect::<DbResult<Vec<_>>>()?;

        let kills =
            sqlx::query("SELECT victim, killer, method, day, at FROM game_kills WHERE game_id = $1 ORDER BY id")
                .bind(game.0)
                .fetch_all(self.store.pool())
                .await?;
        let kills = kills
            .iter()
            .map(|r| {
                let victim: i64 = r.try_get("victim")?;
                let killer: Option<i64> = r.try_get("killer")?;
                let day: i64 = r.try_get("day")?;
                Ok(FixtureKill {
                    killer_telegram_id: killer.unwrap_or(0),
                    victim_telegram_id: victim,
                    killer_name: killer.and_then(|k| names.get(&k).cloned()).unwrap_or_default(),
                    victim_name: names.get(&victim).cloned().unwrap_or_default(),
                    kill_method: r.try_get("method")?,
                    day: day as u32,
                    timestamp: iso_timestamp(r.try_get("at")?),
                })
            })
            .collect::<DbResult<Vec<_>>>()?;

        Ok(FixtureGame {
            game_id: g.try_get("id")?,
            group_name: g.try_get("group_name")?,
            group_telegram_id: g.try_get("group_id")?,
            time_started: iso_timestamp(g.try_get("started_at")?),
            time_ended: ended_at.map(iso_timestamp).unwrap_or_default(),
            winner: enum_to_db(&winner),
            game_mode: g.try_get("mode")?,
            players,
            kills,
        })
    }
}

/// Unix seconds to the fixtures' `2026-07-01T06:45:20.000` format (UTC).
/// Hand-rolled to avoid a date-time dependency for one formatter.
pub(crate) fn iso_timestamp(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_format_like_the_fixtures() {
        assert_eq!(iso_timestamp(0), "1970-01-01T00:00:00.000");
        assert_eq!(iso_timestamp(1_782_888_320), "2026-07-01T06:45:20.000");
        assert_eq!(iso_timestamp(951_782_400), "2000-02-29T00:00:00.000");
    }

    #[test]
    fn results_round_trip() {
        for r in [
            GameResult::Won(Team::Village),
            GameResult::Won(Team::SerialKiller),
            GameResult::Killed,
            GameResult::Interrupted,
            GameResult::Unresolved("hunter standoff".into()),
        ] {
            assert_eq!(GameResult::from_db(&r.to_db()).unwrap(), r);
        }
    }
}
