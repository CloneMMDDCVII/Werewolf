//! Spumpkin's day-phase detonation (Werewolf.cs:2901-2930): pick a target,
//! with a 40% chance (Werewolf.cs:2907) of killing both the target and the
//! Spumpkin at once. There's no per-game usage gate in the legacy code —
//! `spumpkin.Choice` resets every day (Werewolf.cs:5116-5130) so a failed
//! (60%) attempt can simply try again tomorrow — same "no limit, the coin
//! flip is the only gate" shape as `chemist::Chemist`, not a one-shot like
//! `gunner::Gunner`'s bullets. The coin flip and mutual-kill resolution
//! are modeled in `orchestrator::resolve_spumpkin_detonation`, not here —
//! including the WiseElder-demotion special case (Werewolf.cs:2916), which
//! turns out not to change lethality at all (the real code still kills
//! both regardless — see that function's doc for why the special case is
//! flavor-only and safe to skip).

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
                vec![DayAction::Detonate {
                    spumpkin: ctx.self_id,
                    target,
                }]
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
                spumpkin: PlayerId(1),
                target: PlayerId(2)
            }]
        );
        assert_eq!(
            spumpkin.day_action(&ctx(Some(PlayerId(2))), &mut state),
            vec![DayAction::Detonate {
                spumpkin: PlayerId(1),
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
