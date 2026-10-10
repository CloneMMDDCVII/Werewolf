//! Wild Child picks a role model on the first night (Werewolf.cs:1757, 1909)
//! and turns Wolf when the role model dies.
//!
//! Resolved in:
//! - [`game::pick_missing_role_models`](crate::game::pick_missing_role_models): a random model when none was chosen;
//! - [`game::apply_transforms`](crate::game::apply_transforms): the transform.

use crate::roles::definition::*;
use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::WildChild,
    behavior: &WildChild,
    night: NightQuestion::RoleModel(Prompt::WildChildRoleModel),
    day: DayQuestion::None,
    info_key: "RoleInfoWildChild",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines {
        eaten: "WildChildEaten",
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 1,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct WildChild;

impl RoleBehavior for WildChild {
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
            toggle_choice: false,
        };
        let mut state = RoleState::default();
        assert_eq!(
            wc.night_action(&ctx, &mut state),
            vec![NightAction::ChooseRoleModel { target: PlayerId(2) }]
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
            toggle_choice: false,
        };
        let mut state = RoleState::default();
        assert_eq!(wc.night_action(&ctx, &mut state), vec![]);
    }
}
