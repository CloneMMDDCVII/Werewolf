//! Sandman chooses, once per game, whether to put everyone to sleep
//! overnight (Werewolf.cs:950-961) — a yes/no toggle, not a target pick,
//! hence `NightContext::toggle_choice` instead of `chosen_target`.
//!
//! What "asleep" suppresses **is** modeled: `orchestrator
//! ::apply_night_results` cancels every death that night the moment it
//! sees a `NightAction::SandmanSleep` in the batch (Werewolf.cs:3011-3020:
//! the whole night function returns early, before any night action
//! resolves). This proof-of-concept still asks every other role their
//! question regardless (there's no cheap way to know Sandman's answer
//! *before* asking everyone else in the same batch), which is a harmless
//! divergence at this fidelity level — nothing here has the other roles'
//! answers do anything but get silently discarded on a sleep night
//! anyway.

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
