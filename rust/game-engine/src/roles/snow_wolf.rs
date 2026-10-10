//! Snow Wolf is on the wolf team but does not vote on the kill
//! (Werewolf.cs:48). Instead it freezes one player each night, who loses their
//! action (`RoleDef::frozen_loses_action`). When no other wolf is left it
//! turns into a Wolf.
//!
//! Resolved in:
//! - [`freeze::freeze_question`](crate::freeze::freeze_question) and [`freeze::apply_freeze`](crate::freeze::apply_freeze): the freeze;
//! - [`transforms::apply_transforms`](crate::transforms::apply_transforms): the transform.

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::SnowWolf,
    behavior: &SnowWolf,
    night: NightQuestion::Freeze,
    day: DayQuestion::None,
    info_key: "RoleInfoSnowWolf",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Itself,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines {
        visited_serial_killer: Some("SnowFrozeKiller"),
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::NonVillage,
        strength: |_| 15,
        pool: Pool::Wolf,
    },
    dealt: true,
};

pub struct SnowWolf;

impl RoleBehavior for SnowWolf {}
