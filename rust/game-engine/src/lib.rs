//! Core game engine. Covers the deterministic majority-path win-condition
//! logic ported from `Werewolf.cs::CheckForWin`/`DoGameEnd` (Werewolf
//! Node/Werewolf.cs:4479-4636), a proof-of-concept role-behavior structure
//! (see `roles` module) for fifteen roles, and a dependency-ordered night
//! orchestrator (see `orchestrator` module) that ties them together behind
//! an abstract `Presenter` trait — this crate never depends on Telegram
//! (or on `sim`), only on the idea that *something* can answer a question.

pub mod game;
pub mod orchestrator;
pub mod roles;

pub use game::{run_game, GameOutcome};

use shared::{KillMethod, Role, Team};

#[derive(Debug, Clone)]
pub struct PlayerState {
    pub id: u64,
    pub role: Role,
    pub alive: bool,
}

#[derive(Debug, Clone)]
pub struct KillEvent {
    pub victim_role: Role,
    pub method: KillMethod,
}

/// Evaluates the winner given the full kill history plus final player
/// states. Currently only adds the Tanner short-circuit
/// (Werewolf.cs:2768-2776: lynching a Tanner ends the game immediately,
/// bypassing the normal win-check dispatch) on top of `evaluate_winner`.
pub fn evaluate_winner_with_kills(players: &[PlayerState], kills: &[KillEvent]) -> WinOutcome {
    if kills
        .iter()
        .any(|k| k.method == KillMethod::Lynch && k.victim_role == Role::Tanner)
    {
        return WinOutcome::Team(Team::Tanner);
    }
    evaluate_winner(players)
}

/// Roles counted as "wolf muscle" for the outnumber check.
/// Mirrors `WolfRoles` array (Werewolf.cs:48) plus the `SnowWolf` special-case
/// that appears alongside it at every call site. Notably excludes `Sorcerer`,
/// which has `Team::Wolf` but is not counted here in the original code.
fn is_wolf_muscle(role: Role) -> bool {
    matches!(
        role,
        Role::Wolf | Role::AlphaWolf | Role::WolfCub | Role::Lycan | Role::SnowWolf
    )
}

/// A resolved winner, or a marker that the original logic branches into
/// territory we haven't ported (RNG outcomes, or state we don't have data
/// for, like InLove/Bullet counts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WinOutcome {
    Team(Team),
    /// e.g. hunter-vs-wolf/SK standoff (Werewolf.cs:4532-4555) — outcome
    /// depends on `Program.R.Next(100) < HunterKillWolfChanceBase`.
    RngDependent(&'static str),
    /// e.g. lovers win, or gunner-parity exception — needs InLove/Bullet
    /// fields our fixture export doesn't currently capture.
    InsufficientData(&'static str),
    /// Multi-step cult auto-convert / 3-way thief-sorcerer-doppelganger
    /// endings (Werewolf.cs:4568-4592, 4700-4806) — not yet ported.
    Unimplemented(&'static str),
}

/// Evaluate the winner from a final (post-game) player snapshot.
/// Mirrors the `switch (alivePlayers.Count())` dispatch in
/// `CheckForWin` (Werewolf.cs:4514-4636), restricted to the branches that
/// are deterministic and don't require InLove/Bullet state.
pub fn evaluate_winner(players: &[PlayerState]) -> WinOutcome {
    let alive: Vec<&PlayerState> = players.iter().filter(|p| p.alive).collect();

    match alive.len() {
        0 => WinOutcome::Team(Team::NoOne),
        1 => {
            let p = alive[0];
            match p.role {
                Role::Tanner | Role::Sorcerer | Role::Thief | Role::Doppelganger => {
                    WinOutcome::Team(Team::NoOne)
                }
                _ => WinOutcome::Team(p.role.team()),
            }
        }
        2 => {
            // Lovers, and the gunner-parity exception below, need InLove/Bullet
            // data we don't have in the fixture export.
            if alive
                .iter()
                .all(|p| matches!(p.role, Role::Sorcerer | Role::Tanner | Role::Thief | Role::Doppelganger))
            {
                return WinOutcome::Team(Team::NoOne);
            }
            if alive.iter().any(|p| p.role == Role::Hunter) {
                return WinOutcome::RngDependent("hunter standoff (Werewolf.cs:4532-4555)");
            }
            if alive.iter().any(|p| p.role == Role::SerialKiller) {
                return WinOutcome::Team(Team::SerialKiller);
            }
            if alive.iter().any(|p| p.role == Role::Arsonist) {
                return WinOutcome::InsufficientData(
                    "arsonist-vs-gunner parity exception (Werewolf.cs:4560)",
                );
            }
            if alive.iter().any(|p| p.role == Role::Cultist) {
                return WinOutcome::Unimplemented("cult auto-convert (Werewolf.cs:4563-4589)");
            }
            evaluate_common(&alive)
        }
        3 => {
            if alive
                .iter()
                .all(|p| matches!(p.role, Role::Sorcerer | Role::Thief | Role::Doppelganger))
            {
                return WinOutcome::Team(Team::NoOne);
            }
            evaluate_common(&alive)
        }
        _ => evaluate_common(&alive),
    }
}

/// The tail shared by every `alivePlayers.Count()` branch after the switch
/// (Werewolf.cs:4599-4635): SK/Arsonist-alive-do-nothing guards, all-cult,
/// wolves-outnumber, all-village.
fn evaluate_common(alive: &[&PlayerState]) -> WinOutcome {
    if alive.iter().any(|p| p.role.team() == Team::SerialKiller) {
        return WinOutcome::Unimplemented("SK still alive among >2 players, game continues");
    }
    if alive.iter().any(|p| p.role.team() == Team::Arsonist) {
        return WinOutcome::Unimplemented("Arsonist still alive among >2 players, game continues");
    }
    if alive.iter().all(|p| p.role.team() == Team::Cult) {
        return WinOutcome::Team(Team::Cult);
    }

    let wolves = alive.iter().filter(|p| is_wolf_muscle(p.role)).count();
    let others = alive.len() - wolves;
    if wolves >= others {
        // Gunner-parity exception (Werewolf.cs:4612-4625) needs Bullet data.
        if alive.iter().any(|p| p.role == Role::Gunner) {
            return WinOutcome::InsufficientData("gunner-parity exception (Werewolf.cs:4612-4625)");
        }
        return WinOutcome::Team(Team::Wolf);
    }

    if alive.iter().all(|p| {
        !is_wolf_muscle(p.role)
            && p.role != Role::Cultist
            && p.role != Role::SerialKiller
            && p.role != Role::Arsonist
    }) {
        return WinOutcome::Team(Team::Village);
    }

    WinOutcome::Unimplemented("no win condition matched, game would continue")
}
