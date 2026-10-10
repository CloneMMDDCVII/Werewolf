//! Role transforms: who changes role when the roster changes (the Snow
//! Wolf, Traitor, Apprentice Seer, Wild Child and Doppelganger), what each
//! change tells the player, and the random role model a Wild Child or
//! Doppelganger gets when they never picked one. [`crate::game`] calls
//! these after each batch of deaths and at the start of each night.

use crate::messages::Note;
use crate::orchestrator::{AlivePlayer, NarrationEvent};
use crate::rng::RngSource;
use crate::role_info::{transform_tells, Cause};
use crate::roles::{PlayerId, RoleState};
use shared::Role;
use std::collections::HashMap;

/// Werewolf.cs:1753-1779: a Wild Child or Doppelganger who didn't pick a
/// role model on the first night is given a random one, and isn't asked
/// again.
pub(crate) fn pick_missing_role_models(
    alive: &[AlivePlayer],
    states: &mut HashMap<PlayerId, RoleState>,
    rng: &mut dyn RngSource,
) -> Vec<NarrationEvent> {
    let mut told = vec![];
    for p in alive
        .iter()
        .filter(|p| matches!(p.role, Role::WildChild | Role::Doppelganger))
    {
        let state = states.entry(p.id).or_default();
        let others: Vec<PlayerId> = alive.iter().map(|o| o.id).filter(|&id| id != p.id).collect();
        if state.remembered_player.is_some() || others.is_empty() {
            continue;
        }
        let target = others[rng.pick_index(others.len())];
        state.remembered_player = Some(target);
        state.primary_used = true;
        told.push(NarrationEvent::Private {
            to: p.id,
            note: Note::RoleModelChosen { target },
        });
    }
    told
}

/// What one of `apply_transforms`' changes tells the player and their
/// teams (see `role_info::transform_tells`).
pub(crate) fn tells_for_transform(
    event: &NarrationEvent,
    alive: &[AlivePlayer],
    states: &HashMap<PlayerId, RoleState>,
    just_died: &[(PlayerId, Role)],
) -> Vec<NarrationEvent> {
    let &NarrationEvent::Transform { player, from, to } = event else {
        return vec![];
    };
    let model = || states.get(&player).and_then(|s| s.remembered_player);
    let cause = match from {
        Role::SnowWolf => Cause::SnowWolfAlone,
        Role::Traitor => Cause::Traitor,
        Role::ApprenticeSeer => Cause::ApprenticeSeer {
            model: just_died
                .iter()
                .find(|&&(_, role)| role == Role::Seer)
                .map(|&(id, _)| id),
        },
        Role::WildChild => match model() {
            Some(model) => Cause::WildChild { model },
            None => return vec![],
        },
        Role::Doppelganger => match model() {
            Some(model) => Cause::Doppelganger { model },
            None => return vec![],
        },
        _ => return vec![],
    };
    // A Doppelganger who copies a Thief is told the plain Thief's role
    // info; deaths don't carry the game's rules this far.
    transform_tells(player, from, to, cause, alive, false)
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
/// - Snow Wolf turns Wolf once no other wolf remains alive
///   (Werewolf.cs:4485-4496).
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
pub(crate) fn apply_transforms(
    alive: &mut [AlivePlayer],
    states: &HashMap<PlayerId, RoleState>,
    just_died: &[(PlayerId, Role)],
) -> Vec<NarrationEvent> {
    let any_wolf_muscle = alive.iter().any(|p| p.role.is_wolf());
    // The legacy `WolfRoles` (Werewolf.cs:48), which leave out the Snow Wolf.
    let any_eating_wolf = alive.iter().any(|p| p.role.is_eating_wolf());
    let any_seer = alive.iter().any(|p| p.role == Role::Seer);
    let alive_ids: Vec<PlayerId> = alive.iter().map(|p| p.id).collect();
    let mut events = vec![];

    for player in alive.iter_mut() {
        match player.role {
            // The last wolf standing stops freezing and starts eating
            // (Werewolf.cs:4485-4496). Checked before the Traitor, who
            // only turns when no Snow Wolf is left either.
            Role::SnowWolf if !any_eating_wolf => {
                events.push(NarrationEvent::Transform {
                    player: player.id,
                    from: player.role,
                    to: Role::Wolf,
                });
                player.role = Role::Wolf;
            }
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
                    if let Some(&(_, model_role)) = just_died.iter().find(|&&(id, _)| id == model) {
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
