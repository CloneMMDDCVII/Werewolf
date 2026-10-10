//! Passive during the night — Traitor has no night action of its own.
//!
//! The real Traitor mechanic (turns Wolf if the last living Wolf dies,
//! Werewolf.cs:4499-4512, at the very start of `CheckForWin`, before the
//! win-check switch statement even runs) **is modeled**, but not in this
//! file: it's a transform triggered by the *alive roster's* composition,
//! not anything Traitor does on its own turn — see
//! `game::apply_transforms`, which runs this check every round rather than
//! reacting to a specific death event (simpler and equivalent: "no wolf
//! muscle currently alive" is the same condition either way). Team here is
//! still just the base (pre-transform) `Role::Traitor.team()`.

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
