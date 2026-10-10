//! Prince has no action. The first time the Prince is lynched they survive
//! and are revealed (Werewolf.cs:2745-2751).
//!
//! Resolved in:
//! - [`orchestrator::apply_day_results`](crate::orchestrator::apply_day_results): the survival;
//! - [`Game::announce_prince`](crate::game::Game::announce_prince): the reveal.

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::Prince,
    behavior: &Prince,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoPrince",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines {
        serial_killed: "PrinceKilled",
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 3,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Prince;

impl RoleBehavior for Prince {}
