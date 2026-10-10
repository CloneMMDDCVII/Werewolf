//! Drunk has no action. Wolves who eat the Drunk lose their second kill and
//! their next night's menu (Werewolf.cs:3321, 5316).
//!
//! Resolved in:
//! - [`Night::wolves_attack`](crate::night::Night::wolves_attack): eating the Drunk;
//! - the night step of [`game::run_game_with_rules`](crate::game::run_game_with_rules): the drunk wolves are
//!   skipped through [`NightMemory::drunk`](crate::night::NightMemory::drunk).

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
