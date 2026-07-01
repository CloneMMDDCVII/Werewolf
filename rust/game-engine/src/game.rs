//! The full loop: night, then day, checking for a winner after each,
//! repeating until one is found or a safety cap is hit. This is the first
//! thing in the crate that ties `orchestrator`'s phase resolution to the
//! `evaluate_winner_with_kills` win-condition logic across *multiple*
//! rounds — until now, every proof (`day_orchestration.rs`,
//! `fixture_day_replay.rs`) checked a single night or a single day in
//! isolation.

use crate::orchestrator::{
    apply_day_results, apply_night_results, resolve_day, resolve_harlot_visit_deaths,
    resolve_hunter_shots, resolve_lover_deaths, resolve_night, resolve_wolf_cub_bonus_kill,
    AlivePlayer, NarrationEvent, Presenter,
};
use crate::roles::{DayAction, LoversState, NightAction, PlayerId, RoleState};
use crate::{evaluate_winner_with_kills, is_wolf_muscle, KillEvent, PlayerState, WinOutcome};
use shared::{KillMethod, Role, Team};
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
    let mut lovers = LoversState::default();
    // Set by a Blacksmith's SpreadSilver the day before, consumed by the
    // *next* night's apply_night_results call, then reset - cross-round
    // state the same way LoversState is, just a plain bool instead of a
    // whole struct since there's only ever one thing to remember.
    let mut silver_spread_tonight = false;

    for round in 1..=max_rounds {
        let (night_actions, wolf_target) = resolve_night(&alive, &mut states, presenter).await;
        for action in &night_actions {
            if let NightAction::LinkLovers { a, b } = action {
                lovers.link(*a, *b);
            }
        }
        apply_thief_steals(&night_actions, &mut alive, presenter).await;
        let mut night_deaths = apply_night_results(&night_actions, wolf_target, silver_spread_tonight);
        night_deaths.extend(resolve_harlot_visit_deaths(&night_actions, wolf_target));
        let (night_deaths, night_died_with_roles) =
            process_deaths(&night_deaths, &mut alive, &states, &lovers, &mut kills, &mut deaths, presenter).await;
        maybe_wolf_cub_bonus_kill(&night_died_with_roles, &mut alive, &states, &lovers, &mut kills, &mut deaths, presenter).await;
        let night_shots = resolve_hunter_shots(&night_deaths, &night_died_with_roles, &alive, presenter).await;
        process_deaths(&night_shots, &mut alive, &states, &lovers, &mut kills, &mut deaths, presenter).await;

        if let Some(outcome) = resolved_winner(&alive, &kills, &lovers) {
            presenter.narrate(NarrationEvent::GameOver { winner: outcome.clone() }).await;
            return GameOutcome {
                winner: outcome,
                rounds_played: round,
                deaths,
            };
        }

        let (day_actions, lynch_target) = resolve_day(&alive, &mut states, presenter).await;
        let lynch_target_role = lynch_target.and_then(|t| alive.iter().find(|p| p.id == t).map(|p| p.role));
        demote_gunner_if_shot_a_wise_elder(&day_actions, &mut alive, presenter).await;
        silver_spread_tonight = day_actions.iter().any(|a| matches!(a, DayAction::SpreadSilver));
        let day_deaths = apply_day_results(&day_actions, lynch_target, lynch_target_role, &mut states);
        let (day_deaths, day_died_with_roles) =
            process_deaths(&day_deaths, &mut alive, &states, &lovers, &mut kills, &mut deaths, presenter).await;
        maybe_wolf_cub_bonus_kill(&day_died_with_roles, &mut alive, &states, &lovers, &mut kills, &mut deaths, presenter).await;
        let day_shots = resolve_hunter_shots(&day_deaths, &day_died_with_roles, &alive, presenter).await;
        process_deaths(&day_shots, &mut alive, &states, &lovers, &mut kills, &mut deaths, presenter).await;

        if let Some(outcome) = resolved_winner(&alive, &kills, &lovers) {
            presenter.narrate(NarrationEvent::GameOver { winner: outcome.clone() }).await;
            return GameOutcome {
                winner: outcome,
                rounds_played: round,
                deaths,
            };
        }

        presenter.advance_round();
    }

    let winner = WinOutcome::Unimplemented(
        "max_rounds reached without a resolved winner (likely repeated tied votes)",
    );
    presenter.narrate(NarrationEvent::GameOver { winner: winner.clone() }).await;
    GameOutcome {
        winner,
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
/// Returns the same (victim, role-at-death) pairs for `apply_transforms`'
/// Doppelganger case, which needs to know not just *that* its role model
/// died but *what they were*.
fn record_deaths(
    still_alive: &[AlivePlayer],
    new_deaths: &[(PlayerId, KillMethod)],
    kills: &mut Vec<KillEvent>,
    deaths: &mut Vec<(PlayerId, KillMethod)>,
) -> Vec<(PlayerId, Role)> {
    let mut died_with_roles = vec![];
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
        died_with_roles.push((victim, victim_role));
    }
    deaths.extend(new_deaths.iter().copied());
    died_with_roles
}

/// Tells the presenter about every death from this phase, in order, right
/// after they're resolved — not saved up and replayed from `GameOutcome`
/// once the whole game is over. `new_deaths` and `died_with_roles` come
/// from the same `record_deaths` call in the same order, so zipping them
/// is safe; kept as a separate pass (rather than folded into
/// `record_deaths` itself) since "build the win-check bookkeeping" and
/// "tell the presenter" are different concerns that happen to run over
/// the same data.
async fn narrate_deaths(
    new_deaths: &[(PlayerId, KillMethod)],
    died_with_roles: &[(PlayerId, Role)],
    presenter: &mut dyn Presenter,
) {
    for (&(victim, method), &(_, role)) in new_deaths.iter().zip(died_with_roles) {
        presenter
            .narrate(NarrationEvent::Death { victim, role, method })
            .await;
    }
}

/// Records, narrates, removes from `alive`, and resolves any transform
/// triggered by one batch of deaths - the full sequence every source of
/// deaths needs (night, day, and now Hunter's revenge shot), pulled into
/// one place instead of copy-pasted a third time.
///
/// Also folds in `resolve_lover_deaths` right at the start, rather than
/// leaving each call site to remember it - Werewolf.cs's lover check
/// lives inside the one, universal `KillPlayer` function
/// (Werewolf.cs:5609-5616), so it applies to *every* death regardless of
/// source, the same way it needs to apply here whether the batch came
/// from a night, a lynch, or a Hunter's revenge shot.
///
/// Returns the full death batch actually processed (original deaths plus
/// any chained lover death) alongside the (victim, role-at-death) pairs,
/// so a caller that needs either for something *else* deaths trigger
/// (`resolve_hunter_shots`, which needs both the method - for which
/// prompt to ask - and the role - to find a Hunter) doesn't have to
/// re-derive them.
async fn process_deaths(
    new_deaths: &[(PlayerId, KillMethod)],
    alive: &mut Vec<AlivePlayer>,
    states: &HashMap<PlayerId, RoleState>,
    lovers: &LoversState,
    kills: &mut Vec<KillEvent>,
    deaths: &mut Vec<(PlayerId, KillMethod)>,
    presenter: &mut dyn Presenter,
) -> (Vec<(PlayerId, KillMethod)>, Vec<(PlayerId, Role)>) {
    let lover_chain = resolve_lover_deaths(new_deaths, lovers, alive);
    let mut all_deaths = new_deaths.to_vec();
    all_deaths.extend(lover_chain);

    let died_with_roles = record_deaths(alive, &all_deaths, kills, deaths);
    narrate_deaths(&all_deaths, &died_with_roles, presenter).await;
    alive.retain(|p| !all_deaths.iter().any(|&(v, _)| v == p.id));
    let transforms = apply_transforms(alive, states, &died_with_roles);
    narrate_all(transforms, presenter).await;
    (all_deaths, died_with_roles)
}

/// Tells the presenter about a batch of already-built events, in order.
/// Shared by anything that (unlike `narrate_deaths`) can build its
/// `NarrationEvent`s directly rather than zipping two parallel arrays —
/// `apply_transforms` today, the natural landing spot for anything else
/// that ends up producing a batch of events at once.
async fn narrate_all(events: Vec<NarrationEvent>, presenter: &mut dyn Presenter) {
    for event in events {
        presenter.narrate(event).await;
    }
}

/// The Wise Elder's real defensive quirk (Werewolf.cs:2887-2889): shooting
/// one doesn't save them — they still die from the shot like anyone else
/// (confirmed directly: `KillPlayer` runs unconditionally right after the
/// Wise Elder switch case) — but it demotes the *Gunner* to Villager as a
/// consequence. Checked and applied before `apply_day_results` turns
/// `day_actions` into deaths, using the still-intact `alive` roster to
/// look up the target's role; `DayAction::Shoot`'s `shooter` field is
/// what makes finding the right player to demote a lookup instead of a
/// guess (at most one Gunner exists in practice, but there's no reason to
/// assume that when the action already names who fired).
async fn demote_gunner_if_shot_a_wise_elder(
    day_actions: &[DayAction],
    alive: &mut [AlivePlayer],
    presenter: &mut dyn Presenter,
) {
    let shooter = day_actions.iter().find_map(|a| match a {
        DayAction::Shoot { shooter, target } => {
            let target_role = alive.iter().find(|p| p.id == *target).map(|p| p.role);
            (target_role == Some(Role::WiseElder)).then_some(*shooter)
        }
        _ => None,
    });
    let Some(shooter) = shooter else { return };
    if let Some(gunner) = alive.iter_mut().find(|p| p.id == shooter) {
        let from = gunner.role;
        presenter
            .narrate(NarrationEvent::Transform {
                player: gunner.id,
                from,
                to: Role::Villager,
            })
            .await;
        gunner.role = Role::Villager;
    }
}

/// Applies the Thief's steal, immediately, the moment `resolve_night`
/// produces it — unlike Doppelganger's `ChooseRoleModel` (which waits on
/// a death), the swap is unconditional and instant (Werewolf.cs:2483-
/// 2486): the Thief becomes the target's role, and the target becomes a
/// Villager. Applied before `apply_night_results` computes deaths, same
/// timing as extracting Cupid's `LinkLovers` right above this call site -
/// neither depends on who dies tonight.
async fn apply_thief_steals(
    night_actions: &[NightAction],
    alive: &mut [AlivePlayer],
    presenter: &mut dyn Presenter,
) {
    for action in night_actions {
        let NightAction::StealRole { thief, target } = action else {
            continue;
        };
        let target_role = alive.iter().find(|p| p.id == *target).map(|p| p.role);
        let Some(target_role) = target_role else { continue };
        let thief_role = alive.iter().find(|p| p.id == *thief).map(|p| p.role);
        let Some(thief_role) = thief_role else { continue };

        presenter
            .narrate(NarrationEvent::Transform {
                player: *target,
                from: target_role,
                to: Role::Villager,
            })
            .await;
        presenter
            .narrate(NarrationEvent::Transform {
                player: *thief,
                from: thief_role,
                to: target_role,
            })
            .await;

        if let Some(p) = alive.iter_mut().find(|p| p.id == *target) {
            p.role = Role::Villager;
        }
        if let Some(p) = alive.iter_mut().find(|p| p.id == *thief) {
            p.role = target_role;
        }
    }
}

/// If a Wolf Cub is among the roles that just died, the wolves get one
/// bonus kill (Werewolf.cs:1047-1053, `WolfCubKilled`) - resolved through
/// the same death pipeline (`process_deaths`) as anything else, so it
/// gets recorded, narrated, and can trigger its own transforms exactly
/// like a normal kill.
///
/// **Timing simplification**: the legacy code applies this within the
/// same night's resolution when the Wolf Cub dies at night, but the
/// *following* night when it dies during the day (the bonus menu is only
/// ever sent as part of `SendNightActions`). This proof-of-concept
/// applies it immediately regardless of which phase triggered it, rather
/// than threading another cross-round flag through for a day-death case -
/// a Wolf Cub dying to a lynch or a Gunner's shot is the less common
/// path, and the difference is only *when* the bonus kill lands, not
/// whether it does.
async fn maybe_wolf_cub_bonus_kill(
    died_with_roles: &[(PlayerId, Role)],
    alive: &mut Vec<AlivePlayer>,
    states: &HashMap<PlayerId, RoleState>,
    lovers: &LoversState,
    kills: &mut Vec<KillEvent>,
    deaths: &mut Vec<(PlayerId, KillMethod)>,
    presenter: &mut dyn Presenter,
) {
    if !died_with_roles.iter().any(|&(_, role)| role == Role::WolfCub) {
        return;
    }
    if let Some(target) = resolve_wolf_cub_bonus_kill(alive, presenter).await {
        process_deaths(&[(target, KillMethod::Eat)], alive, states, lovers, kills, deaths, presenter).await;
    }
}

/// Applies role transforms. Most are triggered by the current alive
/// roster's composition rather than by reacting to one specific death
/// event — "no wolf muscle currently alive," "no Seer currently alive,"
/// "role model not in the alive list" are the same check either way, and
/// evaluating them fresh each round is simpler than threading through
/// exactly which death caused it. Doppelganger is the one exception that
/// needs to know more than "did my role model die" — it needs *what they
/// were*, hence `just_died`.
///
/// - Traitor turns Wolf once no wolf-muscle role remains alive
///   (Werewolf.cs:4499-4512).
/// - Apprentice Seer turns Seer once no Seer remains alive
///   (Werewolf.cs:4053).
/// - Wild Child turns Wolf once her remembered role model (see
///   `roles::wild_child`) is no longer among the alive.
/// - Doppelganger copies whatever role their remembered role model had
///   the moment they died (Werewolf.cs:1936-1937) — not always Wolf,
///   unlike Wild Child.
///
/// Idempotent: once a player's role has been transformed away from the
/// one a given branch matches on, that branch doesn't match them again.
///
/// Returns every transform that happened, as `NarrationEvent`s, for the
/// caller to narrate — same shape as `record_deaths` returning
/// `(PlayerId, Role)` pairs rather than narrating inline: building the
/// event and mutating `player.role` are one step here (the `from` has to
/// be captured before it's overwritten), but *telling* a presenter is a
/// separate concern the caller (`run_game`) owns, same as it does for
/// deaths.
fn apply_transforms(
    alive: &mut [AlivePlayer],
    states: &HashMap<PlayerId, RoleState>,
    just_died: &[(PlayerId, Role)],
) -> Vec<NarrationEvent> {
    let any_wolf_muscle = alive.iter().any(|p| is_wolf_muscle(p.role));
    let any_seer = alive.iter().any(|p| p.role == Role::Seer);
    let alive_ids: Vec<PlayerId> = alive.iter().map(|p| p.id).collect();
    let mut events = vec![];

    for player in alive.iter_mut() {
        match player.role {
            Role::Traitor if !any_wolf_muscle => {
                events.push(NarrationEvent::Transform {
                    player: player.id,
                    from: player.role,
                    to: Role::Wolf,
                });
                player.role = Role::Wolf;
            }
            Role::ApprenticeSeer if !any_seer => {
                events.push(NarrationEvent::Transform {
                    player: player.id,
                    from: player.role,
                    to: Role::Seer,
                });
                player.role = Role::Seer;
            }
            Role::WildChild => {
                if let Some(model) = states.get(&player.id).and_then(|s| s.remembered_player) {
                    if !alive_ids.contains(&model) {
                        events.push(NarrationEvent::Transform {
                            player: player.id,
                            from: player.role,
                            to: Role::Wolf,
                        });
                        player.role = Role::Wolf;
                    }
                }
            }
            Role::Doppelganger => {
                if let Some(model) = states.get(&player.id).and_then(|s| s.remembered_player) {
                    if let Some(&(_, model_role)) = just_died.iter().find(|&&(id, _)| id == model)
                    {
                        events.push(NarrationEvent::Transform {
                            player: player.id,
                            from: player.role,
                            to: model_role,
                        });
                        player.role = model_role;
                    }
                }
            }
            _ => {}
        }
    }
    events
}

/// `Some(outcome)` only for an actual `Team` win — every other `WinOutcome`
/// variant means "don't stop, but also don't pretend this was a clean
/// continue" (see `run_game`'s doc comment on the RNG/data-gap caveat).
/// Only needs the *current* alive roster: `evaluate_winner` filters to
/// `alive` players internally, so dead players' entries would never be
/// looked at anyway.
///
/// Checks a Lovers win *before* the normal team logic below, matching the
/// real precedence (Werewolf.cs:4525-4527: the lovers check is the very
/// first thing `CheckForWin`'s two-player case looks at). Getting this
/// order backwards is a real bug this test setup caught: two mutually
/// in-love Villager survivors would otherwise hit `evaluate_common`'s "no
/// wolves alive" branch and resolve to a plain Village win before the
/// Lovers check ever ran. `evaluate_winner_with_kills` itself has no idea
/// lover pairings exist at all (that's why `sim`'s fixture replay can
/// never verify a Lovers outcome — the historical export has no `InLove`
/// data to give it), so `run_game` is the one place with enough live
/// state to check it. The Tanner-lynch short-circuit inside
/// `evaluate_winner_with_kills` still can't conflict with this ordering:
/// it only ever names someone who just died, never one of the two
/// players a Lovers win would be checking.
fn resolved_winner(alive: &[AlivePlayer], kills: &[KillEvent], lovers: &LoversState) -> Option<WinOutcome> {
    let player_states: Vec<PlayerState> = alive
        .iter()
        .map(|p| PlayerState {
            id: p.id.0,
            role: p.role,
            alive: true,
        })
        .collect();

    // Checked *before* the general team logic below, matching
    // Werewolf.cs:4525-4527 - the lovers check is the very first thing
    // `CheckForWin`'s two-player case looks at, ahead of even the
    // Sorcerer/Tanner/Thief/Doppelgänger tie-check right after it.
    // `evaluate_common`'s "no wolves alive" branch would otherwise call
    // two mutually-in-love Villager survivors a plain Village win, which
    // is the bug this ordering exists to avoid.
    let alive_ids: Vec<PlayerId> = alive.iter().map(|p| p.id).collect();
    if lovers.is_lovers_win(&alive_ids) {
        return Some(WinOutcome::Team(Team::Lovers));
    }

    if let outcome @ WinOutcome::Team(_) = evaluate_winner_with_kills(&player_states, kills) {
        return Some(outcome);
    }

    None
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
        let events = apply_transforms(&mut alive, &HashMap::new(), &[]);
        assert_eq!(alive[0].role, Role::Wolf);
        assert_eq!(
            events,
            vec![NarrationEvent::Transform {
                player: PlayerId(1),
                from: Role::Traitor,
                to: Role::Wolf,
            }]
        );
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
        apply_transforms(&mut alive, &HashMap::new(), &[]);
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
        apply_transforms(&mut alive, &states, &[]);
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
        apply_transforms(&mut alive, &states, &[]);
        assert_eq!(alive[0].role, Role::WildChild);
    }

    #[test]
    fn apprentice_seer_becomes_seer_once_no_seer_remains() {
        let mut alive = vec![
            AlivePlayer {
                id: PlayerId(1),
                role: Role::ApprenticeSeer,
            },
            AlivePlayer {
                id: PlayerId(2),
                role: Role::Villager,
            },
        ];
        apply_transforms(&mut alive, &HashMap::new(), &[]);
        assert_eq!(alive[0].role, Role::Seer);
    }

    #[test]
    fn apprentice_seer_stays_apprentice_while_the_seer_lives() {
        let mut alive = vec![
            AlivePlayer {
                id: PlayerId(1),
                role: Role::ApprenticeSeer,
            },
            AlivePlayer {
                id: PlayerId(2),
                role: Role::Seer,
            },
        ];
        apply_transforms(&mut alive, &HashMap::new(), &[]);
        assert_eq!(alive[0].role, Role::ApprenticeSeer);
    }

    #[test]
    fn doppelganger_copies_the_dead_role_models_actual_role() {
        let doppelganger = PlayerId(1);
        let role_model = PlayerId(2);
        let mut alive = vec![AlivePlayer {
            id: doppelganger,
            role: Role::Doppelganger,
        }];
        let mut states = HashMap::new();
        states.insert(
            doppelganger,
            RoleState {
                remembered_player: Some(role_model),
                ..Default::default()
            },
        );
        let just_died = [(role_model, Role::SerialKiller)];
        let events = apply_transforms(&mut alive, &states, &just_died);
        assert_eq!(alive[0].role, Role::SerialKiller);
        assert_eq!(
            events,
            vec![NarrationEvent::Transform {
                player: doppelganger,
                from: Role::Doppelganger,
                to: Role::SerialKiller,
            }]
        );
    }

    #[test]
    fn doppelganger_stays_itself_if_someone_else_died() {
        let doppelganger = PlayerId(1);
        let role_model = PlayerId(2);
        let mut alive = vec![AlivePlayer {
            id: doppelganger,
            role: Role::Doppelganger,
        }];
        let mut states = HashMap::new();
        states.insert(
            doppelganger,
            RoleState {
                remembered_player: Some(role_model),
                ..Default::default()
            },
        );
        let just_died = [(PlayerId(3), Role::SerialKiller)]; // not the role model
        apply_transforms(&mut alive, &states, &just_died);
        assert_eq!(alive[0].role, Role::Doppelganger);
    }
}
