//! The Cultist Hunter's hunt and the full-mode Thief's nightly steal, both
//! of which go through the legacy `VisitPlayer` (Werewolf.cs:2323-2430)
//! before anything happens.
//!
//! Only the branches these two visitors can reach are ported: a dead
//! target, the Serial Killer, a wolf (the full Thief only), a Grave Digger
//! who dug last night, and a Harlot or Guardian Angel who went out.
//! Burning targets are left out because the Arsonist's douse does not set
//! the legacy `Burning` flag in this port.

use crate::is_wolf_muscle;
use crate::messages::{DiggerVisitor, Note};
use crate::orchestrator::{grave_digger_fall_chance_percent, AlivePlayer, NarrationEvent};
use crate::rng::RngSource;
use crate::roles::{NightAction, PlayerId};
use shared::{KillMethod, Role, Team};

/// `Settings.ThiefStealChance` (Settings.cs, release build).
pub const THIEF_STEAL_CHANCE_PERCENT: u32 = 50;

/// What the night looks like when the visit happens.
pub struct VisitScene<'a> {
    pub alive: &'a [AlivePlayer],
    /// Everyone already killed tonight, in any way.
    pub dead_tonight: &'a [PlayerId],
    /// Tonight's actions with their actors, after the freeze.
    pub actions: &'a [(PlayerId, NightAction)],
    pub frozen: Option<PlayerId>,
    pub dug_graves: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Visit {
    Success,
    VisitorDied,
    Fail,
    AlreadyDead,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct VisitOutcome {
    pub deaths: Vec<(PlayerId, KillMethod)>,
    pub events: Vec<NarrationEvent>,
    /// A full-mode steal that landed: `(thief, victim)`.
    pub steal: Option<(PlayerId, PlayerId)>,
}

impl VisitOutcome {
    fn tell(&mut self, to: PlayerId, note: Note) {
        self.events.push(NarrationEvent::Private { to, note });
    }
}

fn role_of(scene: &VisitScene, id: PlayerId) -> Option<Role> {
    scene.alive.iter().find(|p| p.id == id).map(|p| p.role)
}

fn went_out(scene: &VisitScene, id: PlayerId) -> bool {
    scene.actions.iter().any(|(actor, a)| *actor == id && matches!(a, NightAction::Visit { .. } | NightAction::Protect { .. }))
}

/// `VisitPlayer` for a Cultist Hunter or a full-mode Thief.
fn visit(visitor: PlayerId, visitor_role: Role, target: PlayerId, scene: &VisitScene, rng: &mut dyn RngSource, outcome: &mut VisitOutcome) -> Visit {
    let Some(target_role) = role_of(scene, target) else {
        return Visit::AlreadyDead;
    };
    if scene.dead_tonight.contains(&target) {
        return Visit::AlreadyDead;
    }
    if target_role == Role::SerialKiller {
        outcome.deaths.push((visitor, KillMethod::VisitKiller));
        if visitor_role == Role::Thief {
            outcome.tell(visitor, Note::StealKiller);
        }
        return Visit::VisitorDied;
    }
    if visitor_role == Role::Thief && is_wolf_muscle(target_role) {
        return Visit::Fail;
    }
    if target_role == Role::GraveDigger && scene.dug_graves > 0 {
        let village = visitor_role.team() == Team::Village;
        if rng.chance(grave_digger_fall_chance_percent(scene.dug_graves, village)) {
            outcome.deaths.push((visitor, KillMethod::FallGrave));
            let by = if visitor_role == Role::Thief { DiggerVisitor::Thief } else { DiggerVisitor::CultistHunter };
            outcome.tell(target, Note::FellDigger { visitor, by });
            if visitor_role == Role::Thief {
                outcome.tell(visitor, Note::ThiefFell { target });
            }
            return Visit::VisitorDied;
        }
        return Visit::Fail;
    }
    if matches!(target_role, Role::Harlot | Role::GuardianAngel) && went_out(scene, target) && scene.frozen != Some(target) {
        return Visit::Fail;
    }
    Visit::Success
}

/// Werewolf.cs:3556-3587: the Cultist Hunter kills a Cultist they hunt,
/// and is told what they found either way. A frozen hunter has no action
/// left by now (see `freeze`).
pub fn resolve_hunt(scene: &VisitScene, rng: &mut dyn RngSource) -> VisitOutcome {
    let mut outcome = VisitOutcome::default();
    let hunts: Vec<(PlayerId, PlayerId)> = scene
        .actions
        .iter()
        .filter(|(actor, _)| role_of(scene, *actor) == Some(Role::CultistHunter))
        .filter_map(|(actor, a)| match a {
            NightAction::Investigate { target } => Some((*actor, *target)),
            _ => None,
        })
        .collect();
    for (hunter, target) in hunts {
        if scene.dead_tonight.contains(&hunter) {
            continue;
        }
        match visit(hunter, Role::CultistHunter, target, scene, rng, &mut outcome) {
            Visit::Success if role_of(scene, target) == Some(Role::Cultist) => {
                outcome.tell(hunter, Note::HunterFindCultist { target });
                outcome.tell(target, Note::CHHuntedYou);
                outcome.deaths.push((target, KillMethod::Hunt));
            }
            Visit::Success | Visit::Fail => outcome.tell(hunter, Note::HunterFailedToFind { target }),
            Visit::AlreadyDead => outcome.tell(hunter, Note::HunterVisitDead { target }),
            Visit::VisitorDied => {}
        }
    }
    outcome
}

/// Werewolf.cs:4166-4189: the full-mode Thief's steal, at the end of the
/// night. It lands half the time, never on a wolf, Cultist or Snow Wolf.
pub fn resolve_full_steal(scene: &VisitScene, rng: &mut dyn RngSource) -> VisitOutcome {
    let mut outcome = VisitOutcome::default();
    let steal = scene.actions.iter().find_map(|(_, a)| match a {
        NightAction::StealRole { thief, target } => Some((*thief, *target)),
        _ => None,
    });
    let Some((thief, target)) = steal else {
        return outcome;
    };
    if scene.dead_tonight.contains(&thief) || role_of(scene, thief) != Some(Role::Thief) {
        return outcome;
    }
    match visit(thief, Role::Thief, target, scene, rng, &mut outcome) {
        Visit::Success => {
            let target_role = role_of(scene, target).expect("a successful visit is to a living player");
            let lands = rng.chance(THIEF_STEAL_CHANCE_PERCENT);
            if lands && !is_wolf_muscle(target_role) && target_role != Role::Cultist {
                outcome.steal = Some((thief, target));
            } else {
                outcome.tell(thief, Note::ThiefStealFailed { target });
            }
        }
        Visit::Fail => outcome.tell(thief, Note::ThiefStealFailed { target }),
        Visit::AlreadyDead => outcome.tell(thief, Note::ThiefStealDead { target }),
        Visit::VisitorDied => {}
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::ScriptedRngSource;

    const A: PlayerId = PlayerId(1);
    const B: PlayerId = PlayerId(2);

    fn rolls(chances: &[bool]) -> ScriptedRngSource {
        chances.iter().fold(ScriptedRngSource::new(), |rng, &c| rng.with_chance(c))
    }

    fn pair(a: Role, b: Role) -> Vec<AlivePlayer> {
        vec![AlivePlayer { id: A, role: a }, AlivePlayer { id: B, role: b }]
    }

    fn scene<'a>(alive: &'a [AlivePlayer], actions: &'a [(PlayerId, NightAction)]) -> VisitScene<'a> {
        VisitScene { alive, dead_tonight: &[], actions, frozen: None, dug_graves: 0 }
    }

    fn steal() -> Vec<(PlayerId, NightAction)> {
        vec![(A, NightAction::StealRole { thief: A, target: B })]
    }

    fn hunt() -> Vec<(PlayerId, NightAction)> {
        vec![(A, NightAction::Investigate { target: B })]
    }

    fn told(outcome: &VisitOutcome) -> Vec<Note> {
        outcome
            .events
            .iter()
            .filter_map(|e| match e {
                NarrationEvent::Private { note, .. } => Some(*note),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_steal_lands_on_a_good_roll() {
        let alive = pair(Role::Thief, Role::Seer);
        let actions = steal();
        let out = resolve_full_steal(&scene(&alive, &actions), &mut rolls(&[true]));
        assert_eq!(out.steal, Some((A, B)));
        let out = resolve_full_steal(&scene(&alive, &actions), &mut rolls(&[false]));
        assert_eq!((out.steal, told(&out)), (None, vec![Note::ThiefStealFailed { target: B }]));
    }

    #[test]
    fn wolves_and_cultists_cannot_be_stolen_from() {
        let actions = steal();
        for role in [Role::Wolf, Role::SnowWolf] {
            let alive = pair(Role::Thief, role);
            let out = resolve_full_steal(&scene(&alive, &actions), &mut rolls(&[]));
            assert_eq!(told(&out), vec![Note::ThiefStealFailed { target: B }]);
        }
        let alive = pair(Role::Thief, Role::Cultist);
        let out = resolve_full_steal(&scene(&alive, &actions), &mut rolls(&[true]));
        assert_eq!((out.steal, told(&out)), (None, vec![Note::ThiefStealFailed { target: B }]));
    }

    #[test]
    fn a_harlot_who_went_out_is_not_home() {
        let alive = pair(Role::Thief, Role::Harlot);
        let mut actions = steal();
        actions.push((B, NightAction::Visit { visitor: B, target: A }));
        let out = resolve_full_steal(&scene(&alive, &actions), &mut rolls(&[]));
        assert_eq!(told(&out), vec![Note::ThiefStealFailed { target: B }]);
    }

    #[test]
    fn the_serial_killer_kills_a_visiting_thief_or_hunter() {
        let alive = pair(Role::Thief, Role::SerialKiller);
        let actions = steal();
        let out = resolve_full_steal(&scene(&alive, &actions), &mut rolls(&[]));
        assert_eq!(out.deaths, vec![(A, KillMethod::VisitKiller)]);
        assert_eq!(told(&out), vec![Note::StealKiller]);

        let alive = pair(Role::CultistHunter, Role::SerialKiller);
        let actions = hunt();
        let out = resolve_hunt(&scene(&alive, &actions), &mut rolls(&[]));
        assert_eq!((out.deaths.clone(), told(&out)), (vec![(A, KillMethod::VisitKiller)], vec![]));
    }

    #[test]
    fn a_dead_target_is_reported() {
        let alive = pair(Role::Thief, Role::Seer);
        let actions = steal();
        let mut s = scene(&alive, &actions);
        s.dead_tonight = &[B];
        let out = resolve_full_steal(&s, &mut rolls(&[]));
        assert_eq!(told(&out), vec![Note::ThiefStealDead { target: B }]);
    }

    #[test]
    fn a_grave_digger_can_trap_the_thief() {
        let alive = pair(Role::Thief, Role::GraveDigger);
        let actions = steal();
        let mut s = scene(&alive, &actions);
        s.dug_graves = 1;
        let out = resolve_full_steal(&s, &mut rolls(&[true]));
        assert_eq!(out.deaths, vec![(A, KillMethod::FallGrave)]);
        assert_eq!(told(&out), vec![Note::FellDigger { visitor: A, by: DiggerVisitor::Thief }, Note::ThiefFell { target: B }]);
        let out = resolve_full_steal(&s, &mut rolls(&[false]));
        assert_eq!(told(&out), vec![Note::ThiefStealFailed { target: B }]);
    }

    #[test]
    fn the_cultist_hunter_kills_a_cultist() {
        let alive = pair(Role::CultistHunter, Role::Cultist);
        let actions = hunt();
        let out = resolve_hunt(&scene(&alive, &actions), &mut rolls(&[]));
        assert_eq!(out.deaths, vec![(B, KillMethod::Hunt)]);
        assert_eq!(told(&out), vec![Note::HunterFindCultist { target: B }, Note::CHHuntedYou]);
    }

    #[test]
    fn the_cultist_hunter_reports_a_miss_or_a_dead_target() {
        let alive = pair(Role::CultistHunter, Role::Wolf);
        let actions = hunt();
        let out = resolve_hunt(&scene(&alive, &actions), &mut rolls(&[]));
        assert_eq!((out.deaths.clone(), told(&out)), (vec![], vec![Note::HunterFailedToFind { target: B }]));
        let mut s = scene(&alive, &actions);
        s.dead_tonight = &[B];
        let out = resolve_hunt(&s, &mut rolls(&[]));
        assert_eq!(told(&out), vec![Note::HunterVisitDead { target: B }]);
    }
}
