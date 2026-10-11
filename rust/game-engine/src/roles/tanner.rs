//! Tanner has no action. Being lynched wins the game for the Tanner
//! (Werewolf.cs:2768-2776).
//!
//! Resolved in:
//! - [`Game::lynched_tanner`](crate::game::Game::lynched_tanner), read by the end-of-game check.

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::Tanner,
    behavior: &Tanner,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoTanner",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines::DEFAULT,
    balance: Balance {
        group: BalanceGroup::NonVillage,
        strength,
        pool: Pool::Always,
    },
    dealt: true,
};

/// `GameBalancing.cs::GetStrength`.
fn strength(all_roles: &[Role]) -> i32 {
    all_roles.len() as i32 / 2
}

pub struct Tanner;

impl RoleBehavior for Tanner {}
