//! Passive during the night — Masons have no night action. The real
//! ability (knowing who the other Masons are, Werewolf.cs:1660-1664) is
//! purely informational, same category as `beholder::Beholder`. Nothing
//! to model beyond `team()`.

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
