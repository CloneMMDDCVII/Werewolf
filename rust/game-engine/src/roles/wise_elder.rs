//! Wise Elder has no action. The wolves' first attack fails
//! (Werewolf.cs:3392-3406); the second kills. A Gunner's shot kills the Wise
//! Elder and demotes the Gunner to Villager (Werewolf.cs:2887-2895). With
//! roles shown, a Chemist who kills the Wise Elder becomes a Villager too
//! (Werewolf.cs:4276-4281).
//!
//! Resolved in:
//! - [`Night::wolves_attack`](crate::night::Night::wolves_attack): surviving the first attack;
//! - [`game::demote_gunner_if_shot_a_wise_elder`](crate::game::demote_gunner_if_shot_a_wise_elder);
//! - [`Night::chemist_killed_wise_elder`](crate::night::Night::chemist_killed_wise_elder).

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::WiseElder,
    behavior: &WiseElder,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoWiseElder",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 30,
    frozen_loses_action: false,
    deaths: DeathLines::DEFAULT,
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 3,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct WiseElder;

impl RoleBehavior for WiseElder {}
