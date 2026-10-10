//! Passive during the night — Tanner has no night action at all.
//!
//! Tanner's actual win condition (wins the instant they're lynched,
//! overriding everything else — Werewolf.cs:2768-2776) is **already
//! implemented**, just not in this trait: see
//! `game_engine::evaluate_winner_with_kills`, which short-circuits to
//! `Team::Tanner` on any `KillMethod::Lynch` against a `Role::Tanner`
//! victim. Not duplicated here to avoid two sources of truth for the same
//! rule.

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
