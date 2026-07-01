//! Lycan is wolf-team muscle (`WolfRoles`, Werewolf.cs:48) and shares the
//! same night eat vote as `wolf::Wolf`.
//!
//! The real distinguishing quirk — Seer/Sorcerer checks show Lycan as a
//! Villager instead of revealing the wolf team (Werewolf.cs:3961-3962,
//! the "sneaky wuff" comment) — is purely about what information an
//! `Investigate`/`CheckTeam` reveals, which this proof-of-concept already
//! treats as unresolved display-layer detail for every investigate-shaped
//! role (see `seer`/`sorcerer` module docs). Nothing extra to model here.
//! Team is already correct via `is_wolf_muscle`.

use crate::roles::{NightAction, NightContext, PlayerId, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Lycan;

impl RoleBehavior for Lycan {
    fn team(&self) -> Team {
        Role::Lycan.team()
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
    fn lycan_votes_for_a_valid_alive_target() {
        let lycan = Lycan;
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
            lycan.night_action(&ctx, &mut state),
            vec![NightAction::EatVote {
                target: PlayerId(2)
            }]
        );
    }
}
