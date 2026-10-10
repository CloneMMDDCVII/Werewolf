//! The Snow Wolf's freeze (Werewolf.cs:3092-3184, the "Snow Wolf Night"
//! region, which runs before every other night action resolves). A frozen
//! player's action fails *that same night*: every later region checks
//! `!x.Frozen` before acting. The flag is cleared after the next night's
//! menus go out (Werewolf.cs:5335-5342), and the freeze menu is built
//! before that reset, so the same player can't be frozen two nights in a
//! row (Werewolf.cs:5276).
//!
//! Only the regions that actually check `Frozen` are blocked: Harlot,
//! Chemist, Cultist, Cultist Hunter, Seer, Sorcerer, Fool, Oracle, Augur,
//! Guardian Angel, Serial Killer and the Grave Digger's graves. The
//! Arsonist ignores it, the non-full Thief steals regardless
//! (Werewolf.cs:4136) while a full-mode Thief is stopped, and wolves can't be targeted at all.

use crate::messages::{FrozenNotice, Note};
use crate::orchestrator::{AlivePlayer, NarrationEvent, Presenter, Prompt};
use crate::rng::RngSource;
use crate::roles::{NightAction, PlayerId};
use shared::{KillMethod, Role};

/// The legacy `WolfRoles` array (Werewolf.cs:48). The Snow Wolf's targets
/// exclude these but not other Snow Wolves or the Sorcerer.
fn is_legacy_wolf_role(role: Role) -> bool {
    matches!(role, Role::Wolf | Role::AlphaWolf | Role::WolfCub | Role::Lycan)
}

/// Whether being frozen cancels this role's night action.
fn freeze_blocks(role: Role) -> bool {
    matches!(
        role,
        Role::Harlot
            | Role::Chemist
            | Role::Cultist
            | Role::CultistHunter
            | Role::Seer
            | Role::Sorcerer
            | Role::Fool
            | Role::Oracle
            | Role::Augur
            | Role::GuardianAngel
            | Role::SerialKiller
            | Role::GraveDigger
    )
}

/// Chance the Snow Wolf dies visiting a Serial Killer who has a target
/// tonight (Werewolf.cs:2342: `Program.R.Next(100) < 80`). With no target
/// the visit is always fatal.
const SNOW_WOLF_DIES_AT_SERIAL_KILLER_PERCENT: u32 = 80;

/// Chance a frozen Hunter is actually frozen rather than shooting the
/// Snow Wolf (Werewolf.cs:3114: `Program.R.Next(100) < 50`).
const HUNTER_FROZEN_PERCENT: u32 = 50;

/// Who would be asked to freeze tonight and who they may pick: the living
/// Snow Wolf, and everyone but wolves and last night's frozen player
/// (Werewolf.cs:5274-5276). `None` when there's nobody to ask.
pub fn freeze_question(
    alive: &[AlivePlayer],
    frozen_last_night: Option<PlayerId>,
    silver_spread: bool,
) -> Option<(PlayerId, Vec<PlayerId>)> {
    if silver_spread {
        return None;
    }
    let snow_wolf = alive.iter().find(|p| p.role == Role::SnowWolf)?.id;
    let options: Vec<PlayerId> = alive
        .iter()
        .filter(|p| p.id != snow_wolf && !is_legacy_wolf_role(p.role) && Some(p.id) != frozen_last_night)
        .map(|p| p.id)
        .collect();
    Some((snow_wolf, options))
}

/// Asks the living Snow Wolf who to freeze tonight. Nobody is asked the
/// night after a Blacksmith spreads silver (Werewolf.cs:5274:
/// `if (!_silverSpread)`). Returns `(snow_wolf, target)`.
pub async fn ask_freeze(
    alive: &[AlivePlayer],
    frozen_last_night: Option<PlayerId>,
    silver_spread: bool,
    presenter: &mut dyn Presenter,
) -> Option<(PlayerId, PlayerId)> {
    let (snow_wolf, options) = freeze_question(alive, frozen_last_night, silver_spread)?;
    let picked = presenter
        .ask_targets(snow_wolf, Prompt::SnowWolfFreeze, &options, 1)
        .await?;
    match picked.as_slice() {
        [target] if options.contains(target) => Some((snow_wolf, *target)),
        _ => None,
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct FreezeOutcome {
    /// Who ended up frozen, for blocking their result messages tonight and
    /// keeping them out of tomorrow's freeze menu.
    pub frozen: Option<PlayerId>,
    /// Private notices for the target and the Snow Wolf.
    pub events: Vec<NarrationEvent>,
    /// The Snow Wolf, if the visit killed them.
    pub deaths: Vec<(PlayerId, KillMethod)>,
    /// Whoever a Guardian Angel kept from being frozen, who counts as
    /// saved tonight (Werewolf.cs:3109).
    pub saved: Option<PlayerId>,
}

/// Resolves the freeze picked by `ask_freeze` and removes the frozen
/// player's actions from `actions` where the freeze blocks them.
/// `dug_graves` is only used to pick the Grave Digger's notice; the
/// caller zeroes the graves themselves.
pub fn apply_freeze(
    freeze: Option<(PlayerId, PlayerId)>,
    actions: &mut Vec<(PlayerId, NightAction)>,
    alive: &[AlivePlayer],
    dug_graves: u32,
    thief_full: bool,
    rng: &mut dyn RngSource,
) -> FreezeOutcome {
    let mut outcome = FreezeOutcome::default();
    let Some((snow_wolf, target)) = freeze else {
        return outcome;
    };
    let Some(target_role) = alive.iter().find(|p| p.id == target).map(|p| p.role) else {
        return outcome;
    };
    let tell = |to, note| NarrationEvent::Private { to, note };

    let notice = match target_role {
        Role::SerialKiller => {
            // VisitPlayer's Serial Killer gate runs first (Werewolf.cs:2340-2348).
            let has_target = actions
                .iter()
                .any(|(actor, a)| *actor == target && matches!(a, NightAction::SerialKillVote { .. }));
            if !has_target || rng.chance(SNOW_WOLF_DIES_AT_SERIAL_KILLER_PERCENT) {
                outcome.deaths.push((snow_wolf, KillMethod::VisitKiller));
                return outcome;
            }
            FrozenNotice::SerialKiller
        }
        _ if actions
            .iter()
            .any(|(_, a)| matches!(a, NightAction::Protect { target: t } if *t == target)) =>
        {
            outcome
                .events
                .push(tell(snow_wolf, Note::GuardBlockedSnowWolf { target }));
            outcome.saved = Some(target);
            return outcome;
        }
        Role::Hunter => {
            if !rng.chance(HUNTER_FROZEN_PERCENT) {
                outcome.deaths.push((snow_wolf, KillMethod::HunterShot));
                return outcome;
            }
            FrozenNotice::Default
        }
        Role::Harlot => FrozenNotice::Harlot,
        Role::Chemist => FrozenNotice::Chemist,
        Role::Cultist => FrozenNotice::Cultist,
        Role::CultistHunter => FrozenNotice::CultistHunter,
        Role::Seer | Role::Sorcerer | Role::Fool | Role::Oracle | Role::Augur => FrozenNotice::Seeing,
        Role::GuardianAngel => FrozenNotice::GuardianAngel,
        Role::GraveDigger if dug_graves > 0 => FrozenNotice::GraveDigger,
        Role::Arsonist => FrozenNotice::ArsonistNotFrozen,
        Role::Thief if thief_full => FrozenNotice::Thief,
        _ => FrozenNotice::Default,
    };

    outcome.frozen = Some(target);
    if freeze_blocks(target_role) || (target_role == Role::Thief && thief_full) {
        actions.retain(|(actor, _)| *actor != target);
    }
    outcome.events.push(tell(target, Note::Frozen(notice)));
    outcome.events.push(tell(snow_wolf, Note::SuccessfulFreeze { target }));
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::ScriptedRngSource;
    use async_trait::async_trait;

    const SW: PlayerId = PlayerId(1);
    const T: PlayerId = PlayerId(2);

    fn roster(target_role: Role) -> Vec<AlivePlayer> {
        vec![
            AlivePlayer {
                id: SW,
                role: Role::SnowWolf,
            },
            AlivePlayer {
                id: T,
                role: target_role,
            },
            AlivePlayer {
                id: PlayerId(3),
                role: Role::Wolf,
            },
            AlivePlayer {
                id: PlayerId(4),
                role: Role::GuardianAngel,
            },
        ]
    }

    fn private(to: PlayerId, note: Note) -> NarrationEvent {
        NarrationEvent::Private { to, note }
    }

    #[test]
    fn a_frozen_seer_loses_their_reading() {
        let mut actions = vec![(T, NightAction::CheckTeam { target: PlayerId(3) })];
        let out = apply_freeze(
            Some((SW, T)),
            &mut actions,
            &roster(Role::Seer),
            0,
            false,
            &mut ScriptedRngSource::new(),
        );
        assert!(actions.is_empty());
        assert_eq!(out.frozen, Some(T));
        assert_eq!(
            out.events,
            vec![
                private(T, Note::Frozen(FrozenNotice::Seeing)),
                private(SW, Note::SuccessfulFreeze { target: T })
            ]
        );
    }

    #[test]
    fn a_frozen_harlot_stays_home() {
        let mut actions = vec![(
            T,
            NightAction::Visit {
                visitor: T,
                target: PlayerId(3),
            },
        )];
        let out = apply_freeze(
            Some((SW, T)),
            &mut actions,
            &roster(Role::Harlot),
            0,
            false,
            &mut ScriptedRngSource::new(),
        );
        assert!(actions.is_empty());
        assert_eq!(out.events[0], private(T, Note::Frozen(FrozenNotice::Harlot)));
    }

    #[test]
    fn the_arsonist_acts_despite_the_freeze() {
        let mut actions = vec![(T, NightAction::Douse { target: PlayerId(3) })];
        let out = apply_freeze(
            Some((SW, T)),
            &mut actions,
            &roster(Role::Arsonist),
            0,
            false,
            &mut ScriptedRngSource::new(),
        );
        assert_eq!(actions.len(), 1);
        assert_eq!(out.frozen, Some(T), "still counts as frozen for tomorrow's menu");
        assert_eq!(out.events[0], private(T, Note::Frozen(FrozenNotice::ArsonistNotFrozen)));
    }

    #[test]
    fn a_guardian_angel_blocks_the_freeze() {
        let mut actions = vec![
            (PlayerId(4), NightAction::Protect { target: T }),
            (T, NightAction::CheckTeam { target: PlayerId(3) }),
        ];
        let out = apply_freeze(
            Some((SW, T)),
            &mut actions,
            &roster(Role::Seer),
            0,
            false,
            &mut ScriptedRngSource::new(),
        );
        assert_eq!(actions.len(), 2);
        assert_eq!(out.frozen, None);
        assert_eq!(out.events, vec![private(SW, Note::GuardBlockedSnowWolf { target: T })]);
    }

    #[test]
    fn a_frozen_guardian_angel_protects_nobody() {
        let ga = PlayerId(4);
        let mut actions = vec![(ga, NightAction::Protect { target: PlayerId(3) })];
        let out = apply_freeze(
            Some((SW, ga)),
            &mut actions,
            &roster(Role::Villager),
            0,
            false,
            &mut ScriptedRngSource::new(),
        );
        assert!(actions.is_empty());
        assert_eq!(out.events[0], private(ga, Note::Frozen(FrozenNotice::GuardianAngel)));
    }

    #[test]
    fn the_serial_killer_is_frozen_when_the_snow_wolf_survives_the_visit() {
        let mut actions = vec![(T, NightAction::SerialKillVote { target: PlayerId(3) })];
        let mut rng = ScriptedRngSource::new().with_chance(false);
        let out = apply_freeze(
            Some((SW, T)),
            &mut actions,
            &roster(Role::SerialKiller),
            0,
            false,
            &mut rng,
        );
        assert!(actions.is_empty(), "no kill tonight");
        assert_eq!(out.events[0], private(T, Note::Frozen(FrozenNotice::SerialKiller)));
    }

    #[test]
    fn the_snow_wolf_usually_dies_visiting_the_serial_killer() {
        let mut actions = vec![(T, NightAction::SerialKillVote { target: PlayerId(3) })];
        let mut rng = ScriptedRngSource::new().with_chance(true);
        let out = apply_freeze(
            Some((SW, T)),
            &mut actions,
            &roster(Role::SerialKiller),
            0,
            false,
            &mut rng,
        );
        assert_eq!(actions.len(), 1);
        assert_eq!(out.deaths, vec![(SW, KillMethod::VisitKiller)]);
        assert_eq!(out.frozen, None);
    }

    #[test]
    fn a_serial_killer_staying_home_always_kills_the_snow_wolf() {
        let mut actions = vec![];
        let out = apply_freeze(
            Some((SW, T)),
            &mut actions,
            &roster(Role::SerialKiller),
            0,
            false,
            &mut ScriptedRngSource::new(),
        );
        assert_eq!(out.deaths, vec![(SW, KillMethod::VisitKiller)]);
    }

    #[test]
    fn a_hunter_is_frozen_or_shoots_the_snow_wolf() {
        let out = apply_freeze(
            Some((SW, T)),
            &mut vec![],
            &roster(Role::Hunter),
            0,
            false,
            &mut ScriptedRngSource::new().with_chance(true),
        );
        assert_eq!(out.frozen, Some(T));
        let out = apply_freeze(
            Some((SW, T)),
            &mut vec![],
            &roster(Role::Hunter),
            0,
            false,
            &mut ScriptedRngSource::new().with_chance(false),
        );
        assert_eq!(out.deaths, vec![(SW, KillMethod::HunterShot)]);
        assert_eq!(out.frozen, None);
    }

    #[test]
    fn the_grave_digger_notice_depends_on_dug_graves() {
        let out = apply_freeze(
            Some((SW, T)),
            &mut vec![],
            &roster(Role::GraveDigger),
            2,
            false,
            &mut ScriptedRngSource::new(),
        );
        assert_eq!(out.events[0], private(T, Note::Frozen(FrozenNotice::GraveDigger)));
        let out = apply_freeze(
            Some((SW, T)),
            &mut vec![],
            &roster(Role::GraveDigger),
            0,
            false,
            &mut ScriptedRngSource::new(),
        );
        assert_eq!(out.events[0], private(T, Note::Frozen(FrozenNotice::Default)));
    }

    #[test]
    fn only_a_full_mode_thief_is_stopped_by_the_freeze() {
        let steal = || vec![(T, NightAction::StealRole { thief: T, target: SW })];
        let mut actions = steal();
        let out = apply_freeze(
            Some((SW, T)),
            &mut actions,
            &roster(Role::Thief),
            0,
            false,
            &mut ScriptedRngSource::new(),
        );
        assert_eq!(out.events[0], private(T, Note::Frozen(FrozenNotice::Default)));
        assert_eq!(actions.len(), 1);
        let mut actions = steal();
        let out = apply_freeze(
            Some((SW, T)),
            &mut actions,
            &roster(Role::Thief),
            0,
            true,
            &mut ScriptedRngSource::new(),
        );
        assert_eq!(out.events[0], private(T, Note::Frozen(FrozenNotice::Thief)));
        assert!(actions.is_empty());
    }

    struct PickFirst(Vec<Vec<PlayerId>>);

    #[async_trait(?Send)]
    impl Presenter for PickFirst {
        async fn ask_targets(
            &mut self,
            _: PlayerId,
            _: Prompt,
            options: &[PlayerId],
            _: usize,
        ) -> Option<Vec<PlayerId>> {
            self.0.push(options.to_vec());
            options.first().map(|&o| vec![o])
        }
    }

    #[tokio::test]
    async fn the_menu_skips_wolves_and_last_nights_target() {
        let alive = roster(Role::Villager);
        let mut presenter = PickFirst(vec![]);
        let picked = ask_freeze(&alive, Some(T), false, &mut presenter).await;
        assert_eq!(presenter.0, vec![vec![PlayerId(4)]]);
        assert_eq!(picked, Some((SW, PlayerId(4))));
    }

    #[tokio::test]
    async fn silver_means_no_freeze_question() {
        let mut presenter = PickFirst(vec![]);
        assert_eq!(
            ask_freeze(&roster(Role::Villager), None, true, &mut presenter).await,
            None
        );
        assert!(presenter.0.is_empty());
    }
}
