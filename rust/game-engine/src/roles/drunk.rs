//! Passive role, no night action — mechanically identical to `Villager`.
//! In the legacy game the only "Drunk" behavior is a display-layer joke
//! (shown a wrong role hint), which affects messaging, not game state, so
//! there's nothing to port here beyond `team()`.

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::Drunk,
    behavior: &Drunk,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoDrunk",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines {
        eaten: "DrunkEaten",
        serial_killed: "DrunkKilled",
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 3,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Drunk;

impl RoleBehavior for Drunk {}
