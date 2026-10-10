//! Mason has no action. Masons know each other (Werewolf.cs:1660-1664).
//!
//! Resolved in:
//! - [`role_info::role_info`](crate::role_info::role_info): the other Masons' names.

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::Mason,
    behavior: &Mason,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoMason",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines {
        eaten: "MasonEaten",
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::Village,
        strength,
        pool: Pool::Always,
    },
    dealt: true,
};

/// `GameBalancing.cs::GetStrength`.
fn strength(all_roles: &[Role]) -> i32 {
    let count = |r: Role| all_roles.iter().filter(|&&x| x == r).count() as i32;
    let masons = count(Role::Mason);
    if masons <= 1 {
        1
    } else {
        masons + 3
    }
}

pub struct Mason;

impl RoleBehavior for Mason {}
