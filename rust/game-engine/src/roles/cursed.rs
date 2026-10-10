//! Cursed has no action. When the wolves attack the Cursed, they turn Wolf
//! instead of dying (Werewolf.cs:3298-3299).
//!
//! Resolved in:
//! - [`Night::wolves_attack`](crate::night::Night::wolves_attack): the transform.

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::Cursed,
    behavior: &Cursed,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoCursed",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 60,
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
    1 - all_roles.iter().filter(|&&x| x.is_wolf()).count() as i32 / 2
}

pub struct Cursed;

impl RoleBehavior for Cursed {}
