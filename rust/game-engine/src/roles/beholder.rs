//! Beholder has no action. They learn who the Seer is (Werewolf.cs:2055-2110).
//!
//! Resolved in:
//! - [`role_info::role_info`](crate::role_info::role_info): the Seer's name in the role message.

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::Beholder,
    behavior: &Beholder,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoBeholder",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines::DEFAULT,
    balance: Balance {
        group: BalanceGroup::Village,
        strength,
        pool: Pool::Always,
    },
    dealt: true,
};

/// `GameBalancing.cs::GetStrength`.
fn strength(all_roles: &[Role]) -> i32 {
    1 + i32::from(all_roles.contains(&Role::Seer)) * 4 + i32::from(all_roles.contains(&Role::Fool))
}

pub struct Beholder;

impl RoleBehavior for Beholder {}
