//! No night action of her own — Apprentice Seer is a silent backup.
//!
//! The real mechanic (becomes the Seer once the actual Seer dies,
//! Werewolf.cs:4053: `if (roleToSee == IRole.Seer && ... !Players.Any(x
//! => x.PlayerRole == IRole.Seer && !x.IsDead)) roleToSee =
//! IRole.ApprenticeSeer`) **is modeled**, but not in this file — see
//! `game::apply_transforms`, which checks every round whether any Seer is
//! still alive and promotes the Apprentice Seer if not. Same category of
//! mechanic as `traitor::Traitor`'s last-wolf-dies transform: triggered by
//! the alive roster's composition, not anything this role does on its own
//! turn.

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::ApprenticeSeer,
    behavior: &ApprenticeSeer,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoApprenticeSeer",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines {
        eaten: "ApprenticeSeerEaten",
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 6,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct ApprenticeSeer;

impl RoleBehavior for ApprenticeSeer {}
