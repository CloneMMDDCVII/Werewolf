use serde::Deserialize;
use shared::{KillMethod, Role, Winner};
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct FixtureRoot {
    pub games: Vec<RawGame>,
}

#[derive(Debug, Deserialize)]
pub struct RawGame {
    pub game_id: i64,
    pub group_name: String,
    pub group_telegram_id: i64,
    pub time_started: String,
    pub time_ended: String,
    pub winner: String,
    pub game_mode: String,
    pub players: Vec<RawPlayer>,
    pub kills: Vec<RawKill>,
}

#[derive(Debug, Deserialize)]
pub struct RawPlayer {
    pub telegram_id: i64,
    pub name: String,
    pub role: String,
    pub survived: bool,
    pub won: bool,
}

#[derive(Debug, Deserialize)]
pub struct RawKill {
    pub killer_telegram_id: i64,
    pub victim_telegram_id: i64,
    pub killer_name: String,
    pub victim_name: String,
    pub kill_method: String,
    pub day: u32,
    pub timestamp: String,
}

/// A fixture game with strings resolved into `shared` enums.
/// Fails loudly (rather than silently skipping) if the historical data
/// contains a role, kill method, or winner we don't yet model — that's a
/// signal our enums are incomplete, not something to paper over.
#[derive(Debug)]
pub struct GameFixture {
    pub game_id: i64,
    pub winner: Winner,
    pub mode_raw: String,
    pub players: Vec<PlayerFixture>,
    pub kills: Vec<KillFixture>,
}

#[derive(Debug)]
pub struct PlayerFixture {
    pub telegram_id: i64,
    /// Anonymized display name (e.g. `"2B2A"`) — not a real Telegram
    /// handle, safe to print in a transcript. See `RawPlayer::name`.
    pub name: String,
    pub role: Role,
    pub survived: bool,
    pub won: bool,
}

#[derive(Debug)]
pub struct KillFixture {
    pub killer_telegram_id: i64,
    pub victim_telegram_id: i64,
    pub method: KillMethod,
    pub day: u32,
}

#[derive(Debug)]
pub enum FixtureError {
    Io(std::io::Error),
    Json(serde_json::Error),
    UnknownRole { game_id: i64, role: String },
    UnknownKillMethod { game_id: i64, method: String },
    UnknownWinner { game_id: i64, winner: String },
}

impl std::fmt::Display for FixtureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FixtureError::Io(e) => write!(f, "io error: {e}"),
            FixtureError::Json(e) => write!(f, "json error: {e}"),
            FixtureError::UnknownRole { game_id, role } => {
                write!(f, "game {game_id}: unknown role {role:?}")
            }
            FixtureError::UnknownKillMethod { game_id, method } => {
                write!(f, "game {game_id}: unknown kill method {method:?}")
            }
            FixtureError::UnknownWinner { game_id, winner } => {
                write!(f, "game {game_id}: unknown winner {winner:?}")
            }
        }
    }
}

impl std::error::Error for FixtureError {}

fn parse_role(game_id: i64, s: &str) -> Result<Role, FixtureError> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|_| {
        FixtureError::UnknownRole {
            game_id,
            role: s.to_string(),
        }
    })
}

fn parse_kill_method(game_id: i64, s: &str) -> Result<KillMethod, FixtureError> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|_| {
        FixtureError::UnknownKillMethod {
            game_id,
            method: s.to_string(),
        }
    })
}

pub fn load_fixtures(path: impl AsRef<Path>) -> Result<Vec<GameFixture>, FixtureError> {
    let raw = fs::read_to_string(path).map_err(FixtureError::Io)?;
    let root: FixtureRoot = serde_json::from_str(&raw).map_err(FixtureError::Json)?;

    root.games
        .into_iter()
        .map(|g| {
            let winner = Winner::from_legacy_str(&g.winner).ok_or_else(|| {
                FixtureError::UnknownWinner {
                    game_id: g.game_id,
                    winner: g.winner.clone(),
                }
            })?;

            let players = g
                .players
                .into_iter()
                .map(|p| {
                    Ok(PlayerFixture {
                        telegram_id: p.telegram_id,
                        name: p.name,
                        role: parse_role(g.game_id, &p.role)?,
                        survived: p.survived,
                        won: p.won,
                    })
                })
                .collect::<Result<Vec<_>, FixtureError>>()?;

            let kills = g
                .kills
                .into_iter()
                .map(|k| {
                    Ok(KillFixture {
                        killer_telegram_id: k.killer_telegram_id,
                        victim_telegram_id: k.victim_telegram_id,
                        method: parse_kill_method(g.game_id, &k.kill_method)?,
                        day: k.day,
                    })
                })
                .collect::<Result<Vec<_>, FixtureError>>()?;

            Ok(GameFixture {
                game_id: g.game_id,
                winner,
                mode_raw: g.game_mode,
                players,
                kills,
            })
        })
        .collect()
}
