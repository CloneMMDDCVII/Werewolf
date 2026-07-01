//! Sandman chooses, once per game, whether to put everyone to sleep
//! overnight (Werewolf.cs:950-961) — a yes/no toggle, not a target pick,
//! hence `NightContext::toggle_choice` instead of `chosen_target`. What
//! "asleep" actually suppresses is resolution logic this file doesn't
//! attempt.

use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Sandman;

impl RoleBehavior for Sandman {
    fn team(&self) -> Team {
        Role::Sandman.team()
    }

    fn night_action(&self, ctx: &NightContext, state: &mut RoleState) -> Vec<NightAction> {
        if state.primary_used || !ctx.toggle_choice {
            return vec![];
        }
        state.primary_used = true;
        vec![NightAction::SandmanSleep]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::PlayerId;

    fn ctx(toggle_choice: bool) -> NightContext<'static> {
        NightContext {
            alive: &[PlayerId(1), PlayerId(2)][..],
            self_id: PlayerId(1),
            chosen_target: None,
            heal_target: None,
            poison_target: None,
            love_targets: None,
            wolf_target: None,
            toggle_choice,
        }
    }

    #[test]
    fn sandman_can_put_everyone_to_sleep_once() {
        let sandman = Sandman;
        let mut state = RoleState::default();
        assert_eq!(
            sandman.night_action(&ctx(true), &mut state),
            vec![NightAction::SandmanSleep]
        );
        assert!(state.primary_used);
        // Already used - even with toggle_choice true again, no repeat.
        assert_eq!(sandman.night_action(&ctx(true), &mut state), vec![]);
    }

    #[test]
    fn sandman_does_nothing_if_they_decline() {
        let sandman = Sandman;
        let mut state = RoleState::default();
        assert_eq!(sandman.night_action(&ctx(false), &mut state), vec![]);
        assert!(!state.primary_used);
    }
}
