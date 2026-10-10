//! Doppelganger picks a role model on the first night (Werewolf.cs:1927-1938).
//! When the role model dies, the Doppelganger takes their role.
//!
//! Resolved in:
//! - [`game::pick_missing_role_models`](crate::game::pick_missing_role_models): a random model when none was chosen;
//! - [`game::apply_transforms`](crate::game::apply_transforms): copying the dead model's role.

use crate::roles::definition::*;
use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::Doppelganger,
    behavior: &Doppelganger,
    night: NightQuestion::RoleModel(Prompt::DoppelgangerRoleModel),
    day: DayQuestion::None,
    info_key: "RoleInfoDoppelganger",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 0,
    frozen_loses_action: false,
    deaths: DeathLines::DEFAULT,
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 2,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Doppelganger;

impl RoleBehavior for Doppelganger {
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
            vec![NightAction::ChooseRoleModel { target: PlayerId(2) }]
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
