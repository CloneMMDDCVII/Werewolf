//! Guardian Angel protects one player overnight (Werewolf.cs:3091,
//! `AskGuard`). Self-protection is allowed. As with `harlot::Harlot`, the
//! actual protection resolution (does the target survive a wolf attack,
//! does the GA risk dying visiting a wolf — Werewolf.cs:2361-2372) is
//! cross-player resolution logic for a future orchestrator; this file only
//! covers "is this a valid target to declare protection on."

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
        };
        let mut state = RoleState::default();
        assert_eq!(ga.night_action(&ctx, &mut state), vec![]);
    }
}
