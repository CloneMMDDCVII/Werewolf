//! Detective investigates one player by day and learns their exact role
//! (Werewolf.cs:2933-2953), unlike Seer, who only learns a team at night.
//! The legacy game resolves it at the end of the day phase, before the
//! lynch, so this is a `day_action`. The per-day chance that the wolves
//! learn who the Detective is (Werewolf.cs:2937:
//! `Settings.ChanceDetectiveCaught`) and the result message are sent by
//! `reveals::resolve_detective`; this file only covers "is this a valid
//! target to snoop on."

use crate::roles::{DayAction, DayContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Detective;

impl RoleBehavior for Detective {
    fn team(&self) -> Team {
        Role::Detective.team()
    }

    fn day_action(&self, ctx: &DayContext, _state: &mut RoleState) -> Vec<DayAction> {
        match ctx.chosen_target {
            Some(target) if target != ctx.self_id && ctx.alive.contains(&target) => {
                vec![DayAction::Snoop {
                    detective: ctx.self_id,
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

    fn ctx(target: PlayerId) -> DayContext<'static> {
        DayContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: Some(target),
            toggle_choice: false,
        }
    }

    #[test]
    fn detective_snoops_on_a_valid_target() {
        let mut state = RoleState::default();
        assert_eq!(
            Detective.day_action(&ctx(PlayerId(2)), &mut state),
            vec![DayAction::Snoop {
                detective: PlayerId(1),
                target: PlayerId(2)
            }]
        );
    }

    #[test]
    fn detective_cannot_snoop_on_themselves() {
        let mut state = RoleState::default();
        assert_eq!(Detective.day_action(&ctx(PlayerId(1)), &mut state), vec![]);
    }
}
