//! Passive — Prince has no action of their own, day or night. The real
//! ability is entirely reactive: the *first* time Prince is lynched, they
//! survive instead of dying (Werewolf.cs:2745-2751: `!lynched.HasUsedAbility`
//! guards the kill). That's not something Prince decides via `day_action`
//! — it's `orchestrator::apply_day_results` checking, at the moment the
//! lynch would apply, whether the target is a Prince who hasn't spent
//! their immunity yet (`RoleState::primary_used`), same "one-shot survival"
//! shape as Witch's heal canceling the wolf kill.

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::Prince,
    behavior: &Prince,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoPrince",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines {
        serial_killed: "PrinceKilled",
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 3,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Prince;

impl RoleBehavior for Prince {}
