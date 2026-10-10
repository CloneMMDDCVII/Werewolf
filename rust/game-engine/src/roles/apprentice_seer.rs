//! Apprentice Seer has no action. She becomes the Seer once no Seer is alive
//! (Werewolf.cs:4053).
//!
//! Resolved in:
//! - [`transforms::apply_transforms`](crate::transforms::apply_transforms): the promotion, checked every round.

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
