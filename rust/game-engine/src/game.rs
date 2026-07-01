//! The full loop: night, then day, checking for a winner after each,
//! repeating until one is found or a safety cap is hit. This is the first
//! thing in the crate that ties `orchestrator`'s phase resolution to the
//! `evaluate_winner_with_kills` win-condition logic across *multiple*
//! rounds — until now, every proof (`day_orchestration.rs`,
//! `fixture_day_replay.rs`) checked a single night or a single day in
//! isolation.

use crate::orchestrator::{
    apply_day_results, apply_night_results, resolve_day, resolve_night, AlivePlayer, Presenter,
};
use crate::roles::{PlayerId, RoleState};
use crate::{evaluate_winner_with_kills, KillEvent, PlayerState, WinOutcome};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameOutcome {
    pub winner: WinOutcome,
    pub rounds_played: u32,
}

/// Runs the game to completion: night, day, check win, repeat. `max_rounds`
/// is a safety cap, not a rule — a night+day round where both the eat and
/// lynch votes tie produces zero deaths, and without a cap that would loop
/// forever. Hitting the cap returns `WinOutcome::Unimplemented` rather than
/// panicking, since "the vote never resolved" is a real (if unlikely)
/// outcome this proof-of-concept doesn't have a tie-breaker rule for.
///
/// Any `WinOutcome` other than `Team(_)` (i.e. `RngDependent`,
/// `InsufficientData`, `Unimplemented`) is treated as "keep playing," which
/// is honest for `evaluate_common`'s genuine "game continues" cases but
/// **not** for the RNG/data-gap cases — a real game would have resolved
/// those (with a coin flip, say) rather than continuing. That's a known
/// divergence from real play, not a hidden one: those cases already return
/// distinct, named variants specifically so a caller can tell them apart
/// from an actual "no winner yet."
pub async fn run_game(
    players: &[AlivePlayer],
    presenter: &mut dyn Presenter,
    max_rounds: u32,
) -> GameOutcome {
    let mut alive: Vec<AlivePlayer> = players.to_vec();
    let mut states: HashMap<PlayerId, RoleState> = HashMap::new();
    let mut kills: Vec<KillEvent> = vec![];

    let role_of = |id: PlayerId| -> shared::Role {
        players
            .iter()
            .find(|p| p.id == id)
            .expect("a death should only ever name a player who started the game")
            .role
    };

    for round in 1..=max_rounds {
        let (night_actions, wolf_target) = resolve_night(&alive, &mut states, presenter).await;
        let night_deaths = apply_night_results(&night_actions, wolf_target);
        for &(victim, method) in &night_deaths {
            kills.push(KillEvent {
                victim_role: role_of(victim),
                method,
            });
        }
        alive.retain(|p| !night_deaths.iter().any(|&(v, _)| v == p.id));

        if let Some(outcome) = resolved_winner(players, &alive, &kills) {
            return GameOutcome {
                winner: outcome,
                rounds_played: round,
            };
        }

        let (day_actions, lynch_target) = resolve_day(&alive, &mut states, presenter).await;
        let day_deaths = apply_day_results(&day_actions, lynch_target);
        for &(victim, method) in &day_deaths {
            kills.push(KillEvent {
                victim_role: role_of(victim),
                method,
            });
        }
        alive.retain(|p| !day_deaths.iter().any(|&(v, _)| v == p.id));

        if let Some(outcome) = resolved_winner(players, &alive, &kills) {
            return GameOutcome {
                winner: outcome,
                rounds_played: round,
            };
        }
    }

    GameOutcome {
        winner: WinOutcome::Unimplemented(
            "max_rounds reached without a resolved winner (likely repeated tied votes)",
        ),
        rounds_played: max_rounds,
    }
}

/// `Some(outcome)` only for an actual `Team` win — every other `WinOutcome`
/// variant means "don't stop, but also don't pretend this was a clean
/// continue" (see `run_game`'s doc comment on the RNG/data-gap caveat).
fn resolved_winner(
    original: &[AlivePlayer],
    alive: &[AlivePlayer],
    kills: &[KillEvent],
) -> Option<WinOutcome> {
    let player_states: Vec<PlayerState> = original
        .iter()
        .map(|p| PlayerState {
            id: p.id.0,
            role: p.role,
            alive: alive.iter().any(|a| a.id == p.id),
        })
        .collect();

    match evaluate_winner_with_kills(&player_states, kills) {
        outcome @ WinOutcome::Team(_) => Some(outcome),
        _ => None,
    }
}
