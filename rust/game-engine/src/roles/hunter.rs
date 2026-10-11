//! Hunter has no action until they die: then they shoot one player
//! (Werewolf.cs:5420-5484). A pack attacking the Hunter may lose a wolf.
//!
//! Resolved in:
//! - [`orchestrator::deaths::resolve_hunter_shots`](crate::orchestrator::deaths::resolve_hunter_shots): the final shot;
//! - [`Night::wolves_attack`](crate::night::Night::wolves_attack): shooting back at the pack.

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::Hunter,
    behavior: &Hunter,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoHunter",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines::DEFAULT,
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 6,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Hunter;

impl RoleBehavior for Hunter {}
