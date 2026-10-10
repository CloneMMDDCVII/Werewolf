//! AlphaWolf votes with the pack like `wolf::Wolf` (Werewolf.cs:48). When
//! the pack attacks, the Alpha may bite instead of kill (Werewolf.cs:2114-2150):
//! the victim turns Wolf at the start of the next night.
//!
//! Resolved in:
//! - [`Night::wolves_attack`](crate::night::Night::wolves_attack) and [`Night::bite`](crate::night::Night::bite): the bite instead of the kill;
//! - [`game::apply_pending_alpha_bites`](crate::game::apply_pending_alpha_bites): the bitten player's transform.

use crate::roles::definition::*;
use crate::roles::{NightAction, NightContext, PlayerId, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::AlphaWolf,
    behavior: &AlphaWolf,
    night: NightQuestion::Pick(Prompt::WolfEat),
    day: DayQuestion::None,
    info_key: "RoleInfoAlphaWolf",
    seer_sees: SeerSees::As(Role::Wolf),
    sorcerer_sees: SorcererSees::Wolf,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines {
        fell_in_grave: "WolfFellPublic",
        visited_serial_killer: Some("SerialKillerKilledWolf"),
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::NonVillage,
        strength: |_| 12,
        pool: Pool::Wolf,
    },
    dealt: true,
};

pub struct AlphaWolf;

impl RoleBehavior for AlphaWolf {
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
            vec![NightAction::EatVote { target: PlayerId(2) }]
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
