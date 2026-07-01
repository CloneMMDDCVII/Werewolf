//! WolfCub is wolf-team muscle (`WolfRoles`, Werewolf.cs:48) and shares the
//! same night eat vote as `wolf::Wolf`/`alpha_wolf::AlphaWolf` — identical
//! validation logic, same `EatVote` action.
//!
//! The real distinguishing ability (if WolfCub dies, the wolves get a
//! bonus kill the following night, Werewolf.cs:2115-2153) is **not
//! modeled here** — it's a delayed, cross-round effect distinct from
//! anything else built so far. Team is already correct
//! (`shared::Role::WolfCub` maps to `Team::Wolf`, and `is_wolf_muscle`
//! already counts WolfCub as wolf muscle).

use crate::roles::{NightAction, NightContext, PlayerId, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct WolfCub;

impl RoleBehavior for WolfCub {
    fn team(&self) -> Team {
        Role::WolfCub.team()
    }

    fn night_action(&self, ctx: &NightContext, _state: &mut RoleState) -> Vec<NightAction> {
        match ctx.chosen_target {
            Some(target) if is_valid_target(ctx, target) => {
                vec![NightAction::EatVote { target }]
            }
            _ => vec![],
        }
    }
}

fn is_valid_target(ctx: &NightContext, target: PlayerId) -> bool {
    target != ctx.self_id && ctx.alive.contains(&target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wolf_cub_votes_for_a_valid_alive_target() {
        let cub = WolfCub;
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
            cub.night_action(&ctx, &mut state),
            vec![NightAction::EatVote {
                target: PlayerId(2)
            }]
        );
    }
}
