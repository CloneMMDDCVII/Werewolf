//! The Seer checks one player's team each night. Structurally identical to
//! `wolf::Wolf` (validate a chosen target, emit one action) — worth
//! comparing the two files side by side as a second example once you've
//! read `villager.rs`.

use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Seer;

impl RoleBehavior for Seer {
    fn team(&self) -> Team {
        Role::Seer.team()
    }

    fn night_action(&self, ctx: &NightContext, _state: &mut RoleState) -> Vec<NightAction> {
        match ctx.chosen_target {
            Some(target) if target != ctx.self_id && ctx.alive.contains(&target) => {
                vec![NightAction::CheckTeam { target }]
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
    fn seer_checks_a_valid_target() {
        let seer = Seer;
        let ctx = NightContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: Some(PlayerId(2)),
            heal_target: None,
            poison_target: None,
            love_targets: None,
            wolf_target: None,
        };
        let mut state = RoleState::default();
        assert_eq!(
            seer.night_action(&ctx, &mut state),
            vec![NightAction::CheckTeam {
                target: PlayerId(2)
            }]
        );
    }
}
