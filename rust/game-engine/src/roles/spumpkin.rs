//! Spumpkin's day-phase detonation (Werewolf.cs:2901-2930): pick a target,
//! with a 40% chance (Werewolf.cs:2907) of killing both the target and the
//! Spumpkin at once. There's no per-game usage gate in the legacy code —
//! `spumpkin.Choice` resets every day (Werewolf.cs:5116-5130) so a failed
//! (60%) attempt can simply try again tomorrow — same "no limit, the coin
//! flip is the only gate" shape as `chemist::Chemist`, not a one-shot like
//! `gunner::Gunner`'s bullets. The RNG coin flip itself, the mutual-kill
//! resolution, and the WiseElder-demotion special case (Werewolf.cs:2916:
//! detonating a WiseElder turns the Spumpkin into a Villager instead of
//! killing anyone) are all resolution logic for a future orchestrator.

use crate::roles::{DayAction, DayContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Spumpkin;

impl RoleBehavior for Spumpkin {
    fn team(&self) -> Team {
        Role::Spumpkin.team()
    }

    fn day_action(&self, ctx: &DayContext, _state: &mut RoleState) -> Vec<DayAction> {
        match ctx.chosen_target {
            Some(target) if target != ctx.self_id && ctx.alive.contains(&target) => {
                vec![DayAction::Detonate { target }]
            }
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::PlayerId;

    fn ctx(target: Option<PlayerId>) -> DayContext<'static> {
        DayContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: target,
            toggle_choice: false,
        }
    }

    #[test]
    fn spumpkin_can_try_to_detonate_every_day_until_it_lands() {
        let spumpkin = Spumpkin;
        let mut state = RoleState::default();
        assert_eq!(
            spumpkin.day_action(&ctx(Some(PlayerId(2))), &mut state),
            vec![DayAction::Detonate {
                target: PlayerId(2)
            }]
        );
        assert_eq!(
            spumpkin.day_action(&ctx(Some(PlayerId(2))), &mut state),
            vec![DayAction::Detonate {
                target: PlayerId(2)
            }],
            "a failed (60%) attempt isn't a per-game use, unlike Gunner's bullets"
        );
    }

    #[test]
    fn spumpkin_cannot_detonate_itself() {
        let spumpkin = Spumpkin;
        let mut state = RoleState::default();
        assert_eq!(spumpkin.day_action(&ctx(Some(PlayerId(1))), &mut state), vec![]);
    }
}
