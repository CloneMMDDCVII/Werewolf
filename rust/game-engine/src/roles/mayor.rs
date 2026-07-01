//! Mayor chooses whether to publicly reveal their role, once
//! (Werewolf.cs:899-908) — a yes/no toggle, not a target pick, hence
//! `DayContext::toggle_choice`. The reveal itself is purely informational;
//! the real payoff (their lynch vote counting twice afterward,
//! Werewolf.cs:2649-2652) is handled by `orchestrator::resolve_day`
//! directly when tallying votes, since it's a change to vote *weight*,
//! not something this file's `DayAction::Reveal` needs to carry.

use crate::roles::{DayAction, DayContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Mayor;

impl RoleBehavior for Mayor {
    fn team(&self) -> Team {
        Role::Mayor.team()
    }

    fn day_action(&self, ctx: &DayContext, state: &mut RoleState) -> Vec<DayAction> {
        if state.primary_used || !ctx.toggle_choice {
            return vec![];
        }
        state.primary_used = true;
        vec![DayAction::Reveal]
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
    fn mayor_can_reveal_once() {
        let mayor = Mayor;
        let mut state = RoleState::default();
        assert_eq!(mayor.day_action(&ctx(true), &mut state), vec![DayAction::Reveal]);
        assert_eq!(mayor.day_action(&ctx(true), &mut state), vec![]);
    }

    #[test]
    fn mayor_stays_hidden_if_they_decline() {
        let mayor = Mayor;
        let mut state = RoleState::default();
        assert_eq!(mayor.day_action(&ctx(false), &mut state), vec![]);
    }
}
