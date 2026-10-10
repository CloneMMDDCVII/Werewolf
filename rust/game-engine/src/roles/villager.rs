//! Villager has no action and no ability. A new role with no action can start
//! from this file: the default `RoleBehavior` methods do nothing.

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::Villager,
    behavior: &Villager,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoVillager",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines::DEFAULT,
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 1,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Villager;

impl RoleBehavior for Villager {}
