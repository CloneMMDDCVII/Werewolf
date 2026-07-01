//! Wild Child picks a "role model" player once, typically the first night
//! (Werewolf.cs:1757, 1909) — reuses `RoleState::primary_used` the same
//! way `cupid::Cupid` does for its one-shot link, and stashes *who* was
//! picked in `RoleState::remembered_player`.
//!
//! The actual payoff — Wild Child turns Wolf if their role model dies —
//! **is modeled**, but not in this file: it's a transform triggered by
//! someone else's death, which `game::apply_transforms` checks each round
//! (did `remembered_player` drop out of the alive list?) using the value
//! stored here. Same category of mechanic as `traitor::Traitor`'s
//! last-wolf-dies transform; `cursed::Cursed`'s bite-transform is a
//! different, not-yet-modeled trigger (being targeted, not a death) and
//! stays a known gap.

use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct WildChild;

impl RoleBehavior for WildChild {
    fn team(&self) -> Team {
        Role::WildChild.team()
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
    fn wild_child_picks_a_role_model_once() {
        let wc = WildChild;
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
            wc.night_action(&ctx, &mut state),
            vec![NightAction::ChooseRoleModel {
                target: PlayerId(2)
            }]
        );
        assert_eq!(state.remembered_player, Some(PlayerId(2)));
        // Second night: already picked.
        assert_eq!(wc.night_action(&ctx, &mut state), vec![]);
    }

    #[test]
    fn wild_child_cannot_pick_itself_as_role_model() {
        let wc = WildChild;
        let ctx = NightContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: Some(PlayerId(1)),
            heal_target: None,
            poison_target: None,
            love_targets: None,
            wolf_target: None,
        };
        let mut state = RoleState::default();
        assert_eq!(wc.night_action(&ctx, &mut state), vec![]);
    }
}
