//! Doppelganger picks a "role model" player once, typically the first
//! night (Werewolf.cs:1927-1938) — same shape and same `RoleState`
//! fields (`primary_used`, `remembered_player`) as `wild_child::WildChild`.
//!
//! The payoff differs from Wild Child's, though: when the role model dies,
//! Doppelganger doesn't always turn Wolf — they copy whatever role the
//! dead player actually had (`Transform(p, rm.PlayerRole, ...)`,
//! Werewolf.cs:1936-1937). `game::apply_transforms` handles this using the
//! role-at-death information `game::record_deaths` already tracks.

use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Doppelganger;

impl RoleBehavior for Doppelganger {
    fn team(&self) -> Team {
        Role::Doppelganger.team()
    }

    fn night_action(&self, ctx: &NightContext, state: &mut RoleState) -> Vec<NightAction> {
        if state.primary_used {
            return vec![];
        }
        match ctx.chosen_target {
            Some(target) if target != ctx.self_id && ctx.alive.contains(&target) => {
                state.primary_used = true;
                state.remembered_player = Some(target);
                vec![NightAction::ChooseRoleModel { target }]
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
    fn doppelganger_picks_a_role_model_once() {
        let dg = Doppelganger;
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
            dg.night_action(&ctx, &mut state),
            vec![NightAction::ChooseRoleModel {
                target: PlayerId(2)
            }]
        );
        assert_eq!(state.remembered_player, Some(PlayerId(2)));
        assert_eq!(dg.night_action(&ctx, &mut state), vec![]);
    }

    #[test]
    fn doppelganger_cannot_pick_itself_as_role_model() {
        let dg = Doppelganger;
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
        assert_eq!(dg.night_action(&ctx, &mut state), vec![]);
    }
}
