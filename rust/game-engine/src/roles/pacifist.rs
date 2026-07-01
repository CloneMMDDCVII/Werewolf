//! Pacifist chooses whether to veto the day's lynch entirely, once
//! (Werewolf.cs:913-925: `_pacifistUsed = true`) — a yes/no toggle, same
//! shape as `mayor::Mayor`'s reveal. Unlike Reveal, `Pacify` **does**
//! affect a death outcome: `orchestrator::apply_day_results` cancels the
//! resolved lynch target whenever this action is present.

use crate::roles::{DayAction, DayContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Pacifist;

impl RoleBehavior for Pacifist {
    fn team(&self) -> Team {
        Role::Pacifist.team()
    }

    fn day_action(&self, ctx: &DayContext, state: &mut RoleState) -> Vec<DayAction> {
        if state.primary_used || !ctx.toggle_choice {
            return vec![];
        }
        state.primary_used = true;
        vec![DayAction::Pacify]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::PlayerId;

    fn ctx(toggle_choice: bool) -> DayContext<'static> {
        DayContext {
            alive: &[PlayerId(1), PlayerId(2)][..],
            self_id: PlayerId(1),
            chosen_target: None,
            toggle_choice,
        }
    }

    #[test]
    fn pacifist_can_veto_the_lynch_once() {
        let pacifist = Pacifist;
        let mut state = RoleState::default();
        assert_eq!(pacifist.day_action(&ctx(true), &mut state), vec![DayAction::Pacify]);
        assert_eq!(pacifist.day_action(&ctx(true), &mut state), vec![]);
    }

    #[test]
    fn pacifist_lets_the_lynch_happen_if_they_decline() {
        let pacifist = Pacifist;
        let mut state = RoleState::default();
        assert_eq!(pacifist.day_action(&ctx(false), &mut state), vec![]);
    }
}
