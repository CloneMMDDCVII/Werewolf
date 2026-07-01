//! Mirrors the wolves' night-eat vote (Werewolf.cs's wolf night-action
//! handling). Each wolf submits a candidate; the orchestrator (not part of
//! this proof-of-concept) is responsible for tallying votes across all
//! wolves and resolving the actual kill — this file only validates one
//! wolf's own vote.

use crate::roles::{NightAction, NightContext, PlayerId, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Wolf;

impl RoleBehavior for Wolf {
    fn team(&self) -> Team {
        Role::Wolf.team()
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
    fn wolf_cannot_target_itself() {
        let wolf = Wolf;
        let ctx = NightContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: Some(PlayerId(1)),
            heal_target: None,
            poison_target: None,
            love_targets: None,
        };
        let mut state = RoleState::default();
        assert_eq!(wolf.night_action(&ctx, &mut state), vec![]);
    }

    #[test]
    fn wolf_votes_for_a_valid_alive_target() {
        let wolf = Wolf;
        let ctx = NightContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: Some(PlayerId(2)),
            heal_target: None,
            poison_target: None,
            love_targets: None,
        };
        let mut state = RoleState::default();
        assert_eq!(
            wolf.night_action(&ctx, &mut state),
            vec![NightAction::EatVote {
                target: PlayerId(2)
            }]
        );
    }
}
