//! Detective investigates one player overnight and learns their exact role
//! (Werewolf.cs:2933-2953), unlike Seer, who only learns a team. There's
//! also a per-night chance the wolves learn a Detective is active
//! (Werewolf.cs:2937: `Settings.ChanceDetectiveCaught`) — that RNG
//! side-effect is resolution logic for a future orchestrator, not modeled
//! in this file, which only covers "is this a valid target to investigate."

use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Detective;

impl RoleBehavior for Detective {
    fn team(&self) -> Team {
        Role::Detective.team()
    }

    fn night_action(&self, ctx: &NightContext, _state: &mut RoleState) -> Vec<NightAction> {
        match ctx.chosen_target {
            Some(target) if target != ctx.self_id && ctx.alive.contains(&target) => {
                vec![NightAction::Investigate { target }]
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
    fn detective_investigates_a_valid_target() {
        let detective = Detective;
        let ctx = NightContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: Some(PlayerId(2)),
            heal_target: None,
            poison_target: None,
            love_targets: None,
            wolf_target: None,
            toggle_choice: false,
        };
        let mut state = RoleState::default();
        assert_eq!(
            detective.night_action(&ctx, &mut state),
            vec![NightAction::Investigate {
                target: PlayerId(2)
            }]
        );
    }

    #[test]
    fn detective_cannot_investigate_themselves() {
        let detective = Detective;
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
        assert_eq!(detective.night_action(&ctx, &mut state), vec![]);
    }
}
