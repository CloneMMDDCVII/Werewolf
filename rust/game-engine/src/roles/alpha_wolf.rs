//! AlphaWolf is wolf-team muscle (`WolfRoles`, Werewolf.cs:48) and shares
//! the same night eat vote as `wolf::Wolf` — identical validation logic,
//! same `EatVote` action, tallied together by
//! `orchestrator::majority_eat_target`.
//!
//! The real distinguishing ability (a bite/convert mechanic turning a
//! villager into a wolf over subsequent nights, Werewolf.cs:2114-2150) is
//! **not modeled here** — it's a slower, stateful conversion process
//! distinct from anything else built so far, and would need its own
//! dedicated hook. Team is already correct (`shared::Role::AlphaWolf`
//! maps to `Team::Wolf`, and `is_wolf_muscle` already counts AlphaWolf as
//! wolf muscle for win-condition purposes).

use crate::roles::{NightAction, NightContext, PlayerId, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct AlphaWolf;

impl RoleBehavior for AlphaWolf {
    fn team(&self) -> Team {
        Role::AlphaWolf.team()
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
    fn alpha_wolf_votes_for_a_valid_alive_target() {
        let alpha = AlphaWolf;
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
            alpha.night_action(&ctx, &mut state),
            vec![NightAction::EatVote {
                target: PlayerId(2)
            }]
        );
    }

    #[test]
    fn alpha_wolf_cannot_target_itself() {
        let alpha = AlphaWolf;
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
        assert_eq!(alpha.night_action(&ctx, &mut state), vec![]);
    }
}
