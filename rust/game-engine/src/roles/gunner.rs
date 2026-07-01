//! Gunner has two bullets (Werewolf.cs:1938: `bullet: 2`), spent one at a
//! time to shoot a suspected player during the **day** phase
//! (Werewolf.cs:2871-2896) — this is what motivated adding the
//! `day_action` hook: forcing this through `night_action` would have been
//! wrong regardless of how convenient it looked.
//!
//! Reuses `RoleState::primary_used`/`secondary_used` for "first bullet
//! spent"/"second bullet spent", the same trick `witch::Witch` uses for
//! her two potions. What the shot actually resolves to (kill outright,
//! the Wise Elder special-case that reverts Gunner to Villager instead —
//! Werewolf.cs:2887-2889) is resolution logic for a future orchestrator;
//! this file only covers "is there a bullet left, and is this a valid
//! target."

use crate::roles::{DayAction, DayContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Gunner;

impl RoleBehavior for Gunner {
    fn team(&self) -> Team {
        Role::Gunner.team()
    }

    fn day_action(&self, ctx: &DayContext, state: &mut RoleState) -> Vec<DayAction> {
        if state.primary_used && state.secondary_used {
            return vec![];
        }
        match ctx.chosen_target {
            Some(target) if target != ctx.self_id && ctx.alive.contains(&target) => {
                if !state.primary_used {
                    state.primary_used = true;
                } else {
                    state.secondary_used = true;
                }
                vec![DayAction::Shoot { target }]
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
            alive: &[PlayerId(1), PlayerId(2), PlayerId(3)][..],
            self_id: PlayerId(1),
            chosen_target: target,
        }
    }

    #[test]
    fn gunner_can_fire_both_bullets_across_two_days() {
        let gunner = Gunner;
        let mut state = RoleState::default();

        let first = gunner.day_action(&ctx(Some(PlayerId(2))), &mut state);
        assert_eq!(first, vec![DayAction::Shoot { target: PlayerId(2) }]);
        assert!(state.primary_used && !state.secondary_used);

        let second = gunner.day_action(&ctx(Some(PlayerId(3))), &mut state);
        assert_eq!(second, vec![DayAction::Shoot { target: PlayerId(3) }]);
        assert!(state.primary_used && state.secondary_used);
    }

    #[test]
    fn gunner_cannot_fire_a_third_shot() {
        let gunner = Gunner;
        let mut state = RoleState {
            primary_used: true,
            secondary_used: true,
        };
        assert_eq!(gunner.day_action(&ctx(Some(PlayerId(2))), &mut state), vec![]);
    }

    #[test]
    fn gunner_cannot_shoot_itself() {
        let gunner = Gunner;
        let mut state = RoleState::default();
        assert_eq!(gunner.day_action(&ctx(Some(PlayerId(1))), &mut state), vec![]);
    }
}
