//! Guardian Angel protects one player overnight (Werewolf.cs:3091,
//! `AskGuard`). Self-protection is allowed.
//!
//! The core payoff — protecting the wolves' actual target cancels their
//! kill entirely (Werewolf.cs:3280-3286) — **is** modeled, in
//! `orchestrator::apply_night_results`, the same cancellation check
//! Witch's heal potion uses. What's still not modeled: the GA herself
//! risking death instead of the target when they're the same player
//! (Werewolf.cs:2361-2372), and blocking SnowWolf's freeze
//! (Werewolf.cs:3107-3111) — both real, but cross-player resolution logic
//! beyond this one cancellation.

use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct GuardianAngel;

impl RoleBehavior for GuardianAngel {
    fn team(&self) -> Team {
        Role::GuardianAngel.team()
    }

    fn night_action(&self, ctx: &NightContext, _state: &mut RoleState) -> Vec<NightAction> {
        match ctx.chosen_target {
            Some(target) if ctx.alive.contains(&target) => {
                vec![NightAction::Protect { target }]
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
    fn guardian_angel_may_protect_herself() {
        let ga = GuardianAngel;
        let ctx = NightContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: Some(PlayerId(1)),
            heal_target: None,
            poison_target: None,
            love_targets: None,
            wolf_target: None,
            toggle_choice: false,
        };
        let mut state = RoleState::default();
        assert_eq!(
            ga.night_action(&ctx, &mut state),
            vec![NightAction::Protect {
                target: PlayerId(1)
            }]
        );
    }

    #[test]
    fn guardian_angel_cannot_protect_a_dead_player() {
        let ga = GuardianAngel;
        let ctx = NightContext {
            alive: &[PlayerId(1)],
            self_id: PlayerId(1),
            chosen_target: Some(PlayerId(2)),
            heal_target: None,
            poison_target: None,
            love_targets: None,
            wolf_target: None,
            toggle_choice: false,
        };
        let mut state = RoleState::default();
        assert_eq!(ga.night_action(&ctx, &mut state), vec![]);
    }
}
