//! What the information roles learn: the Seer family at night
//! (Werewolf.cs:3922-4062, the "Seer / Fool" and "Augur" regions) and the
//! Detective by day (Werewolf.cs:2933-2953). These regions run after the
//! night's kills, so a reader who died tonight learns nothing, while a
//! target who died tonight is still read.
//!
//! The Cultist Hunter's result belongs with its kill resolution and is
//! not handled here.

use crate::messages::Note;
use crate::orchestrator::{AlivePlayer, NarrationEvent};
use crate::rng::RngSource;
use crate::roles::definition::{SeerSees, SorcererSees};
use crate::roles::{def, DayAction, NightAction, PlayerId};
use shared::Role;

/// Chance the wolves learn who the Detective is on a day they snoop
/// (`Settings.ChanceDetectiveCaught = 40`, Settings.cs:92).
const DETECTIVE_CAUGHT_PERCENT: u32 = 40;

/// Who holds `id`'s role right now: the surviving roster first (it
/// reflects tonight's transforms), then the start-of-night roster for
/// anyone who died tonight.
fn role_of(id: PlayerId, survivors: &[AlivePlayer], start_of_night: &[AlivePlayer]) -> Option<Role> {
    survivors
        .iter()
        .chain(start_of_night)
        .find(|p| p.id == id)
        .map(|p| p.role)
}

/// What the Seer is told about `role` (Werewolf.cs:3934-3953): its
/// `RoleDef::seer_sees`.
fn seer_reading(role: Role, rng: &mut dyn RngSource) -> Role {
    match def(role).seer_sees {
        SeerSees::Itself => role,
        SeerSees::As(shown) => shown,
        SeerSees::WolfOrVillager => {
            if rng.chance(50) {
                Role::Wolf
            } else {
                Role::Villager
            }
        }
    }
}

/// Per-game state the Augur needs across nights (`augur.SawRoles`).
#[derive(Debug, Default, Clone)]
pub struct AugurMemory {
    pub seen: Vec<Role>,
}

/// Sends every Seer, Sorcerer, Fool, Oracle and Augur their reading for
/// tonight, in the legacy order. `actions` must already have frozen
/// players' actions removed (`freeze::apply_freeze`); `frozen` is still
/// needed for the Augur, who has no action to remove. `role_pool` is the
/// pool the game's roles were dealt from (see
/// `balancing::balance_with_pool`).
pub fn resolve_night_readings(
    actions: &[(PlayerId, NightAction)],
    start_of_night: &[AlivePlayer],
    survivors: &[AlivePlayer],
    frozen: Option<PlayerId>,
    role_pool: &[Role],
    augur: &mut AugurMemory,
    rng: &mut dyn RngSource,
) -> Vec<NarrationEvent> {
    let mut events = vec![];
    let survivor_role = |id: PlayerId| survivors.iter().find(|p| p.id == id).map(|p| p.role);
    let tell = |to, note| NarrationEvent::Private { to, note };

    let readings = |role: Role| {
        actions.iter().filter_map(move |&(actor, action)| {
            let target = match action {
                NightAction::CheckTeam { target } | NightAction::Investigate { target } => target,
                _ => return None,
            };
            (survivor_role(actor) == Some(role)).then_some((actor, target))
        })
    };

    for (seer, target) in readings(Role::Seer).collect::<Vec<_>>() {
        let Some(role) = role_of(target, survivors, start_of_night) else {
            continue;
        };
        let role = seer_reading(role, rng);
        events.push(tell(seer, Note::SeerSees { target, role }));
    }

    for (sorcerer, target) in readings(Role::Sorcerer).collect::<Vec<_>>() {
        let Some(role) = role_of(target, survivors, start_of_night) else {
            continue;
        };
        let note = match def(role).sorcerer_sees {
            SorcererSees::Wolf => Note::SeerSees {
                target,
                role: Role::Wolf,
            },
            SorcererSees::Itself => Note::SeerSees { target, role },
            SorcererSees::Nothing => Note::SorcererOther { target },
        };
        events.push(tell(sorcerer, note));
    }

    // The Fool is shown a random living role that isn't a Seer, with
    // wolf types shown as plain Wolf (Werewolf.cs:3991-4016).
    for (fool, target) in readings(Role::Fool).collect::<Vec<_>>() {
        let pool: Vec<Role> = survivors
            .iter()
            .filter(|p| p.id != fool && p.role != Role::Seer)
            .map(|p| p.role)
            .collect();
        if pool.is_empty() {
            continue;
        }
        let mut role = pool[rng.pick_index(pool.len())];
        if role.is_eating_wolf() {
            role = Role::Wolf;
        }
        events.push(tell(fool, Note::SeerSees { target, role }));
    }

    // The Oracle is shown a random living role the target doesn't have
    // (Werewolf.cs:4027-4033), or nothing if every survivor shares it.
    for (oracle, target) in readings(Role::Oracle).collect::<Vec<_>>() {
        let Some(target_role) = role_of(target, survivors, start_of_night) else {
            continue;
        };
        let pool: Vec<Role> = survivors
            .iter()
            .filter(|p| p.id != oracle && p.role != target_role)
            .map(|p| p.role)
            .collect();
        if pool.is_empty() {
            continue;
        }
        let role = pool[rng.pick_index(pool.len())];
        events.push(tell(oracle, Note::NegSeerSees { target, role }));
    }

    // The Augur needs no action: every night they learn one role from the
    // game's pool that no living (or just-died) player holds and they
    // haven't been shown yet (Werewolf.cs:4044-4061).
    if let Some(augur_player) = survivors.iter().find(|p| p.role == Role::Augur) {
        if Some(augur_player.id) != frozen {
            let mut candidates: Vec<Role> = vec![];
            for &role in role_pool {
                let in_game = start_of_night.iter().chain(survivors).any(|p| p.role == role);
                if !in_game && !augur.seen.contains(&role) && !candidates.contains(&role) {
                    candidates.push(role);
                }
            }
            if candidates.is_empty() {
                events.push(tell(augur_player.id, Note::AugurSeesNothing));
            } else {
                let mut role = candidates[rng.pick_index(candidates.len())];
                let seer_alive = survivors.iter().any(|p| p.role == Role::Seer);
                let apprentice_alive = survivors.iter().any(|p| p.role == Role::ApprenticeSeer);
                // About to be replaced by the Apprentice (Werewolf.cs:4053).
                if role == Role::Seer && apprentice_alive && !seer_alive {
                    role = Role::ApprenticeSeer;
                }
                augur.seen.push(role);
                events.push(tell(augur_player.id, Note::AugurSees { role }));
            }
        }
    }

    events
}

/// The Detective's daytime snoop (Werewolf.cs:2933-2953): every wolf has a
/// 40% chance of learning the Detective's name, then the Detective learns
/// the target's exact role. `deaths_before_lynch` are the day's Gunner and
/// Spumpkin kills, which the legacy game resolves just before the snoop, so
/// a Detective shot today learns nothing.
pub fn resolve_detective(
    day_actions: &[DayAction],
    alive: &[AlivePlayer],
    deaths_before_lynch: &[PlayerId],
    rng: &mut dyn RngSource,
) -> Vec<NarrationEvent> {
    let mut events = vec![];
    for action in day_actions {
        let DayAction::Snoop { detective, target } = *action else {
            continue;
        };
        if deaths_before_lynch.contains(&detective) {
            continue;
        }
        let Some(role) = alive.iter().find(|p| p.id == target).map(|p| p.role) else {
            continue;
        };
        if rng.chance(DETECTIVE_CAUGHT_PERCENT) {
            for wolf in alive.iter().filter(|p| p.role.is_wolf()) {
                events.push(NarrationEvent::Private {
                    to: wolf.id,
                    note: Note::DetectiveCaught { detective },
                });
            }
        }
        events.push(NarrationEvent::Private {
            to: detective,
            note: Note::DetectiveSnoop { target, role },
        });
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::ScriptedRngSource;

    fn p(id: u64, role: Role) -> AlivePlayer {
        AlivePlayer { id: PlayerId(id), role }
    }

    fn private(to: u64, note: Note) -> NarrationEvent {
        NarrationEvent::Private { to: PlayerId(to), note }
    }

    fn seer_reads(target_role: Role, rng: &mut dyn RngSource) -> Vec<NarrationEvent> {
        let roster = vec![p(1, Role::Seer), p(2, target_role)];
        let actions = vec![(PlayerId(1), NightAction::CheckTeam { target: PlayerId(2) })];
        resolve_night_readings(&actions, &roster, &roster, None, &[], &mut AugurMemory::default(), rng)
    }

    #[test]
    fn the_seer_sees_true_roles_and_legacy_disguises() {
        for (actual, shown) in [
            (Role::Villager, Role::Villager),
            (Role::Beholder, Role::Beholder),
            (Role::WolfCub, Role::Wolf),
            (Role::AlphaWolf, Role::Wolf),
            (Role::WolfMan, Role::Wolf),
            (Role::Lycan, Role::Villager),
        ] {
            assert_eq!(
                seer_reads(actual, &mut ScriptedRngSource::new()),
                vec![private(
                    1,
                    Note::SeerSees {
                        target: PlayerId(2),
                        role: shown
                    }
                )],
                "{actual:?}"
            );
        }
    }

    #[test]
    fn the_traitor_reads_as_wolf_or_villager() {
        let wolf = seer_reads(Role::Traitor, &mut ScriptedRngSource::new().with_chance(true));
        assert_eq!(
            wolf,
            vec![private(
                1,
                Note::SeerSees {
                    target: PlayerId(2),
                    role: Role::Wolf
                }
            )]
        );
        let villager = seer_reads(Role::Traitor, &mut ScriptedRngSource::new().with_chance(false));
        assert_eq!(
            villager,
            vec![private(
                1,
                Note::SeerSees {
                    target: PlayerId(2),
                    role: Role::Villager
                }
            )]
        );
    }

    #[test]
    fn a_seer_who_died_tonight_learns_nothing() {
        let start = vec![p(1, Role::Seer), p(2, Role::Wolf)];
        let survivors = vec![p(2, Role::Wolf)];
        let actions = vec![(PlayerId(1), NightAction::CheckTeam { target: PlayerId(2) })];
        let events = resolve_night_readings(
            &actions,
            &start,
            &survivors,
            None,
            &[],
            &mut AugurMemory::default(),
            &mut ScriptedRngSource::new(),
        );
        assert!(events.is_empty());
    }

    #[test]
    fn the_sorcerer_only_recognises_wolves_and_seers() {
        let roster = vec![
            p(1, Role::Sorcerer),
            p(2, Role::WolfCub),
            p(3, Role::Seer),
            p(4, Role::SnowWolf),
            p(5, Role::Lycan),
        ];
        for (target, note) in [
            (
                2,
                Note::SeerSees {
                    target: PlayerId(2),
                    role: Role::Wolf,
                },
            ),
            (
                3,
                Note::SeerSees {
                    target: PlayerId(3),
                    role: Role::Seer,
                },
            ),
            (
                4,
                Note::SeerSees {
                    target: PlayerId(4),
                    role: Role::SnowWolf,
                },
            ),
            (5, Note::SorcererOther { target: PlayerId(5) }),
        ] {
            let actions = vec![(
                PlayerId(1),
                NightAction::Investigate {
                    target: PlayerId(target),
                },
            )];
            let events = resolve_night_readings(
                &actions,
                &roster,
                &roster,
                None,
                &[],
                &mut AugurMemory::default(),
                &mut ScriptedRngSource::new(),
            );
            assert_eq!(events, vec![private(1, note)]);
        }
    }

    #[test]
    fn the_fool_sees_a_random_non_seer_role_with_wolf_types_flattened() {
        let roster = vec![
            p(1, Role::Fool),
            p(2, Role::Seer),
            p(3, Role::AlphaWolf),
            p(4, Role::Villager),
        ];
        let actions = vec![(PlayerId(1), NightAction::Investigate { target: PlayerId(4) })];
        // Pool is [AlphaWolf, Villager]; index 0 picks AlphaWolf.
        let events = resolve_night_readings(
            &actions,
            &roster,
            &roster,
            None,
            &[],
            &mut AugurMemory::default(),
            &mut ScriptedRngSource::new().with_index(0),
        );
        assert_eq!(
            events,
            vec![private(
                1,
                Note::SeerSees {
                    target: PlayerId(4),
                    role: Role::Wolf
                }
            )]
        );
    }

    #[test]
    fn the_oracle_sees_a_role_the_target_is_not() {
        let roster = vec![
            p(1, Role::Oracle),
            p(2, Role::Villager),
            p(3, Role::Villager),
            p(4, Role::Wolf),
        ];
        let actions = vec![(PlayerId(1), NightAction::Investigate { target: PlayerId(2) })];
        let events = resolve_night_readings(
            &actions,
            &roster,
            &roster,
            None,
            &[],
            &mut AugurMemory::default(),
            &mut ScriptedRngSource::new().with_index(0),
        );
        assert_eq!(
            events,
            vec![private(
                1,
                Note::NegSeerSees {
                    target: PlayerId(2),
                    role: Role::Wolf
                }
            )]
        );
    }

    #[test]
    fn the_augur_sees_each_absent_role_once_then_nothing() {
        let roster = vec![p(1, Role::Augur), p(2, Role::Wolf)];
        let pool = [Role::Augur, Role::Wolf, Role::Mason, Role::Mason];
        let mut memory = AugurMemory::default();
        let mut rng = ScriptedRngSource::new().with_index(0);
        let first = resolve_night_readings(&[], &roster, &roster, None, &pool, &mut memory, &mut rng);
        assert_eq!(first, vec![private(1, Note::AugurSees { role: Role::Mason })]);
        let second = resolve_night_readings(&[], &roster, &roster, None, &pool, &mut memory, &mut rng);
        assert_eq!(second, vec![private(1, Note::AugurSeesNothing)]);
    }

    #[test]
    fn a_frozen_augur_sees_nothing_at_all() {
        let roster = vec![p(1, Role::Augur)];
        let events = resolve_night_readings(
            &[],
            &roster,
            &roster,
            Some(PlayerId(1)),
            &[Role::Mason],
            &mut AugurMemory::default(),
            &mut ScriptedRngSource::new(),
        );
        assert!(events.is_empty());
    }

    #[test]
    fn the_augur_is_shown_the_apprentice_instead_of_a_dead_seer() {
        let start = vec![p(1, Role::Augur), p(2, Role::ApprenticeSeer)];
        let events = resolve_night_readings(
            &[],
            &start,
            &start,
            None,
            &[Role::Seer],
            &mut AugurMemory::default(),
            &mut ScriptedRngSource::new().with_index(0),
        );
        assert_eq!(
            events,
            vec![private(
                1,
                Note::AugurSees {
                    role: Role::ApprenticeSeer
                }
            )]
        );
    }

    #[test]
    fn the_detective_learns_the_exact_role_and_may_be_caught() {
        let roster = vec![
            p(1, Role::Detective),
            p(2, Role::Lycan),
            p(3, Role::Wolf),
            p(4, Role::SnowWolf),
        ];
        let actions = [DayAction::Snoop {
            detective: PlayerId(1),
            target: PlayerId(2),
        }];
        let caught = resolve_detective(&actions, &roster, &[], &mut ScriptedRngSource::new().with_chance(true));
        assert_eq!(
            caught,
            vec![
                private(2, Note::DetectiveCaught { detective: PlayerId(1) }),
                private(3, Note::DetectiveCaught { detective: PlayerId(1) }),
                private(4, Note::DetectiveCaught { detective: PlayerId(1) }),
                private(
                    1,
                    Note::DetectiveSnoop {
                        target: PlayerId(2),
                        role: Role::Lycan
                    }
                ),
            ]
        );
        let unseen = resolve_detective(&actions, &roster, &[], &mut ScriptedRngSource::new().with_chance(false));
        assert_eq!(
            unseen,
            vec![private(
                1,
                Note::DetectiveSnoop {
                    target: PlayerId(2),
                    role: Role::Lycan
                }
            )]
        );
    }

    #[test]
    fn a_detective_shot_today_learns_nothing() {
        let roster = vec![p(1, Role::Detective), p(2, Role::Wolf)];
        let actions = [DayAction::Snoop {
            detective: PlayerId(1),
            target: PlayerId(2),
        }];
        assert!(resolve_detective(&actions, &roster, &[PlayerId(1)], &mut ScriptedRngSource::new()).is_empty());
    }
}
