//! Blacksmith spreads protective silver to a target during the day
//! (Werewolf.cs:935, 5083) — same "validate a target, emit the action"
//! shape as `gunner::Gunner`'s shot, just once per game
//! (`RoleState::primary_used`) rather than twice. Whether the silver
//! actually protects the target from a wolf attack is resolution logic
//! this file doesn't attempt.

use crate::roles::{DayAction, DayContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Blacksmith;

impl RoleBehavior for Blacksmith {
    fn team(&self) -> Team {
        Role::Blacksmith.team()
    }

    fn day_action(&self, ctx: &DayContext, state: &mut RoleState) -> Vec<DayAction> {
        if state.primary_used {
            return vec![];
        }
        match ctx.chosen_target {
            Some(target) if target != ctx.self_id && ctx.alive.contains(&target) => {
                state.primary_used = true;
                vec![DayAction::SpreadSilver { target }]
            }
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::PlayerId;

    #[test]
    fn blacksmith_spreads_silver_to_a_valid_target_once() {
        let bs = Blacksmith;
        let ctx = DayContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: Some(PlayerId(2)),
            toggle_choice: false,
        };
        let mut state = RoleState::default();
        assert_eq!(
            bs.day_action(&ctx, &mut state),
            vec![DayAction::SpreadSilver {
                target: PlayerId(2)
            }]
        );
        assert_eq!(bs.day_action(&ctx, &mut state), vec![]);
    }
}
