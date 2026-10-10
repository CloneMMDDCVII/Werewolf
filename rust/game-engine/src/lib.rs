//! The game engine: every rule of the legacy game (`Werewolf Node/Werewolf.cs`)
//! with no I/O. A game runs in [`game::run_game_with_rules`] and talks to the
//! outside world only through the [`orchestrator::Presenter`] trait, so
//! this crate depends on neither Telegram nor `sim`.
//!
//! Where things live:
//! - [`roles`]: one file per role, holding its [`roles::RoleDef`];
//! - [`night`]: the night, resolved region by region in the legacy order;
//! - [`orchestrator`]: asking questions, the day, deaths and the hunter's shot;
//! - [`game_end`]: the live end-of-game check;
//! - [`evaluate_winner`] and [`evaluate_winner_with_kills`]: the RNG-free
//!   win check the fixture replays use.
//!
//! `rust/ARCHITECTURE.md` walks through a game and through adding a role.

pub mod balancing;
pub mod death_text;
pub mod freeze;
pub mod game;
pub mod game_end;
pub mod messages;
pub mod night;
pub mod orchestrator;
pub mod player_list;
pub mod reveals;
pub mod rng;
pub mod role_info;
pub mod roles;
pub mod rules;
pub mod summary;

pub use game::{run_game, run_game_with_role_pool, run_game_with_rules, GameOutcome};

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
    /// An admin ended the game outright (`/killgame`, Werewolf.cs
    /// `Kill()`, 5393-5406) — deliberately no winner, not "we don't know
    /// the winner" like the three variants above. The source sends a
    /// literal `"Game removed"` here, not a `GetLocaleString` call, so
    /// unlike every other `WinOutcome` this one has no real locale key
    /// to map to at all (`game_over_locale_key` returns `None`, same as
    /// it does for the other non-`Team` variants, but for a different
    /// reason - there's genuinely nothing to translate, not just
    /// "unmapped yet").
    Killed,
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
                Role::Tanner | Role::Sorcerer | Role::Thief | Role::Doppelganger => WinOutcome::Team(Team::NoOne),
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
                return WinOutcome::InsufficientData("arsonist-vs-gunner parity exception (Werewolf.cs:4560)");
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

    let wolves = alive.iter().filter(|p| p.role.is_wolf()).count();
    let others = alive.len() - wolves;
    if wolves >= others {
        // Gunner-parity exception (Werewolf.cs:4612-4625) needs Bullet data.
        if alive.iter().any(|p| p.role == Role::Gunner) {
            return WinOutcome::InsufficientData("gunner-parity exception (Werewolf.cs:4612-4625)");
        }
        return WinOutcome::Team(Team::Wolf);
    }

    if alive.iter().all(|p| {
        !p.role.is_wolf() && p.role != Role::Cultist && p.role != Role::SerialKiller && p.role != Role::Arsonist
    }) {
        return WinOutcome::Team(Team::Village);
    }

    WinOutcome::Unimplemented("no win condition matched, game would continue")
}
