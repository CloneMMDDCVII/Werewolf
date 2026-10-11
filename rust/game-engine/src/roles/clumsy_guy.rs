//! ClumsyGuy has no action. Half the time their lynch vote lands on a random
//! living player instead (Werewolf.cs:999-1011).
//!
//! Resolved in:
//! - [`orchestrator::day::resolve_day`](crate::orchestrator::day::resolve_day) and
//!   [`orchestrator::day::resolve_lynch_vote`](crate::orchestrator::day::resolve_lynch_vote)
//!   (a Troublemaker's second lynch): the redirected vote.

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::ClumsyGuy,
    behavior: &ClumsyGuy,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoClumsyGuy",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines::DEFAULT,
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| -1,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct ClumsyGuy;

impl RoleBehavior for ClumsyGuy {}
