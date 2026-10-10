//! Traitor has no action. Once no wolf is alive, the Traitor turns Wolf
//! (Werewolf.cs:4499-4512).
//!
//! Resolved in:
//! - [`transforms::apply_transforms`](crate::transforms::apply_transforms): the transform, checked every round.

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::Traitor,
    behavior: &Traitor,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoTraitor",
    seer_sees: SeerSees::WolfOrVillager,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines::DEFAULT,
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 0,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Traitor;

impl RoleBehavior for Traitor {}
