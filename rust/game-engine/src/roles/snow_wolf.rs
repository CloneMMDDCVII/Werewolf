//! SnowWolf is wolf-team muscle for win-condition purposes (`Role::is_wolf`
//! counts it), but it does **not** share the main wolf-eat vote: the
//! legacy vote collection uses `WolfRoles = { Wolf, AlphaWolf, WolfCub,
//! Lycan }` (Werewolf.cs:48), which leaves it out. Its night ability is a
//! freeze instead, asked and resolved by the `freeze` module (it needs the
//! previous night's target and the whole night's actions, neither of
//! which a `NightContext` carries), so this file stays passive. When no
//! other wolf is left it turns into a plain Wolf (`game::apply_transforms`).

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
