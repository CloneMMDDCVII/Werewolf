//! Core game engine. Currently a placeholder — real port of `Werewolf.cs`
//! game logic lands here once the simulation harness can verify it against
//! historical fixtures.

use shared::{KillMethod, Role, Winner};

#[derive(Debug, Clone)]
pub struct PlayerState {
    pub id: u64,
    pub role: Role,
    pub alive: bool,
}

#[derive(Debug, Clone)]
pub struct KillEvent {
    pub killer_id: u64,
    pub victim_id: u64,
    pub method: KillMethod,
    pub day: u32,
}

#[derive(Debug, Clone)]
pub struct GameResult {
    pub winner: Winner,
    pub kills: Vec<KillEvent>,
}
