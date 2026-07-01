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
use crate::{evaluate_winner_with_kills, is_wolf_muscle, KillEvent, PlayerState, WinOutcome};
use shared::{KillMethod, Role};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameOutcome {
    pub winner: WinOutcome,
    pub rounds_played: u32,
    /// Every death, in the order it happened, with the victim's identity —
    /// `KillEvent` (used internally for win-checking) only carries the
    /// victim's role, which isn't enough to compare a simulated game
    /// against a specific historical one death-for-death.
    pub deaths: Vec<(PlayerId, KillMethod)>,
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
    let mut deaths: Vec<(PlayerId, KillMethod)> = vec![];

    for round in 1..=max_rounds {
        let (night_actions, wolf_target) = resolve_night(&alive, &mut states, presenter).await;
        let night_deaths = apply_night_results(&night_actions, wolf_target);
        record_deaths(&alive, &night_deaths, &mut kills, &mut deaths);
        alive.retain(|p| !night_deaths.iter().any(|&(v, _)| v == p.id));
        apply_transforms(&mut alive, &states);

        if let Some(outcome) = resolved_winner(&alive, &kills) {
            return GameOutcome {
                winner: outcome,
                rounds_played: round,
                deaths,
            };
        }

        let (day_actions, lynch_target) = resolve_day(&alive, &mut states, presenter).await;
        let day_deaths = apply_day_results(&day_actions, lynch_target);
        record_deaths(&alive, &day_deaths, &mut kills, &mut deaths);
        alive.retain(|p| !day_deaths.iter().any(|&(v, _)| v == p.id));
        apply_transforms(&mut alive, &states);

        if let Some(outcome) = resolved_winner(&alive, &kills) {
            return GameOutcome {
                winner: outcome,
                rounds_played: round,
                deaths,
            };
        }

        presenter.advance_round();
    }

    GameOutcome {
        winner: WinOutcome::Unimplemented(
            "max_rounds reached without a resolved winner (likely repeated tied votes)",
        ),
        rounds_played: max_rounds,
        deaths,
    }
}

/// Records a batch of deaths into both the win-check kill history and the
/// caller-facing death list, using each victim's role *at the moment they
/// died* — which may already be a transformed role (e.g. a Traitor who
/// turned Wolf in an earlier round) rather than their original assignment.
/// Must be called with `still_alive` from before `.retain()` removes the
/// victims, since that's the only place their pre-death role is available.
fn record_deaths(
    still_alive: &[AlivePlayer],
    new_deaths: &[(PlayerId, KillMethod)],
    kills: &mut Vec<KillEvent>,
    deaths: &mut Vec<(PlayerId, KillMethod)>,
) {
    for &(victim, method) in new_deaths {
        let victim_role = still_alive
            .iter()
            .find(|p| p.id == victim)
            .expect("a death should only ever name a player who was alive to be targeted")
            .role;
        kills.push(KillEvent {
            victim_role,
            method,
        });
    }
    deaths.extend(new_deaths.iter().copied());
}

/// Applies role transforms triggered by the current alive roster's
/// composition rather than by reacting to one specific death event — "no
/// wolf muscle currently alive" and "role model not in the alive list" are
/// the same check either way, and evaluating them fresh each round is
/// simpler than threading through exactly which death caused it.
///
/// - Traitor turns Wolf once no wolf-muscle role remains alive
///   (Werewolf.cs:4499-4512).
/// - Wild Child turns Wolf once her remembered role model (see
///   `roles::wild_child`) is no longer among the alive.
///
/// Idempotent: once a player's role becomes `Role::Wolf`, neither branch
/// matches them again on a later call.
fn apply_transforms(alive: &mut [AlivePlayer], states: &HashMap<PlayerId, RoleState>) {
    let any_wolf_muscle = alive.iter().any(|p| is_wolf_muscle(p.role));
    let alive_ids: Vec<PlayerId> = alive.iter().map(|p| p.id).collect();

    for player in alive.iter_mut() {
        match player.role {
            Role::Traitor if !any_wolf_muscle => {
                player.role = Role::Wolf;
            }
            Role::WildChild => {
                if let Some(model) = states.get(&player.id).and_then(|s| s.remembered_player) {
                    if !alive_ids.contains(&model) {
                        player.role = Role::Wolf;
                    }
                }
            }
            _ => {}
        }
    }
}

/// `Some(outcome)` only for an actual `Team` win — every other `WinOutcome`
/// variant means "don't stop, but also don't pretend this was a clean
/// continue" (see `run_game`'s doc comment on the RNG/data-gap caveat).
/// Only needs the *current* alive roster: `evaluate_winner` filters to
/// `alive` players internally, so dead players' entries would never be
/// looked at anyway.
fn resolved_winner(alive: &[AlivePlayer], kills: &[KillEvent]) -> Option<WinOutcome> {
    let player_states: Vec<PlayerState> = alive
        .iter()
        .map(|p| PlayerState {
            id: p.id.0,
            role: p.role,
            alive: true,
        })
        .collect();

    match evaluate_winner_with_kills(&player_states, kills) {
        outcome @ WinOutcome::Team(_) => Some(outcome),
        _ => None,
    }
}

#[cfg(test)]
mod transform_tests {
    use super::*;

    #[test]
    fn traitor_turns_wolf_once_no_wolf_muscle_remains() {
        let mut alive = vec![
            AlivePlayer {
                id: PlayerId(1),
                role: Role::Traitor,
            },
            AlivePlayer {
                id: PlayerId(2),
                role: Role::Villager,
            },
        ];
        apply_transforms(&mut alive, &HashMap::new());
        assert_eq!(alive[0].role, Role::Wolf);
    }

    #[test]
    fn traitor_stays_a_traitor_while_a_wolf_is_still_alive() {
        let mut alive = vec![
            AlivePlayer {
                id: PlayerId(1),
                role: Role::Traitor,
            },
            AlivePlayer {
                id: PlayerId(2),
                role: Role::Wolf,
            },
        ];
        apply_transforms(&mut alive, &HashMap::new());
        assert_eq!(alive[0].role, Role::Traitor);
    }

    #[test]
    fn wild_child_turns_wolf_once_her_role_model_is_gone() {
        let wild_child = PlayerId(1);
        let mut alive = vec![AlivePlayer {
            id: wild_child,
            role: Role::WildChild,
        }];
        let mut states = HashMap::new();
        states.insert(
            wild_child,
            RoleState {
                remembered_player: Some(PlayerId(99)), // not in `alive`
                ..Default::default()
            },
        );
        apply_transforms(&mut alive, &states);
        assert_eq!(alive[0].role, Role::Wolf);
    }

    #[test]
    fn wild_child_stays_herself_while_her_role_model_is_still_alive() {
        let wild_child = PlayerId(1);
        let role_model = PlayerId(2);
        let mut alive = vec![
            AlivePlayer {
                id: wild_child,
                role: Role::WildChild,
            },
            AlivePlayer {
                id: role_model,
                role: Role::Villager,
            },
        ];
        let mut states = HashMap::new();
        states.insert(
            wild_child,
            RoleState {
                remembered_player: Some(role_model),
                ..Default::default()
            },
        );
        apply_transforms(&mut alive, &states);
        assert_eq!(alive[0].role, Role::WildChild);
    }
}
