//! Passive — ClumsyGuy has no night action. Their lynch vote has a 50%
//! chance of being redirected to a random living player
//! (Werewolf.cs:999-1011); that is vote-tallying interference, so it lives
//! with the vote in `orchestrator::ask_lynch_vote` rather than here.

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
