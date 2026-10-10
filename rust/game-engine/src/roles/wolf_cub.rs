//! Wolf Cub votes with the pack like `wolf::Wolf` (Werewolf.cs:48). When the
//! Wolf Cub dies, the wolves kill twice the next night (Werewolf.cs:1047-1053).
//!
//! Resolved in:
//! - [`game::ask_wolf_cub_votes`](crate::game::ask_wolf_cub_votes): the second vote;
//! - [`Night::wolves`](crate::night::Night::wolves): the second attack.

use crate::roles::definition::*;
use crate::roles::{NightAction, NightContext, PlayerId, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::WolfCub,
    behavior: &WolfCub,
    night: NightQuestion::Pick(Prompt::WolfEat),
    day: DayQuestion::None,
    info_key: "RoleInfoWolfCub",
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
        strength,
        pool: Pool::Wolf,
    },
    dealt: true,
};

/// `GameBalancing.cs::GetStrength`.
fn strength(all_roles: &[Role]) -> i32 {
    let can_have_another_wolf = [
        Role::AlphaWolf,
        Role::Wolf,
        Role::Lycan,
        Role::SnowWolf,
        Role::WildChild,
        Role::Doppelganger,
        Role::Cursed,
        Role::Traitor,
    ]
    .iter()
    .any(|r| all_roles.contains(r));
    if can_have_another_wolf {
        12
    } else {
        10
    }
}

pub struct WolfCub;

impl RoleBehavior for WolfCub {
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
            vec![NightAction::EatVote { target: PlayerId(2) }]
        );
    }
}
