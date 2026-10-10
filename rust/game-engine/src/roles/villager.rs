//! The template for a role that does nothing special. If you're adding a
//! new role and it has no night action, copy this file: rename the type,
//! keep `team()`, delete nothing else. `night_action` is left at its
//! default (no actions) rather than overridden here on purpose, so this
//! stays the shortest possible example of "a role that just exists."

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
