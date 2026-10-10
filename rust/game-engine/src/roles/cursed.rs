//! Passive during the night — Cursed has no night action of its own.
//!
//! The real Cursed mechanic *is* modeled, but not here: like
//! `traitor::Traitor`'s bite conversion, it's a transform triggered by
//! being on the receiving end of the wolves' own night action, not
//! something Cursed decides for itself. Werewolf.cs:3298-3299
//! (`case IRole.Cursed: Transform(target, IRole.Wolf, ...); break;`) shows
//! it's unconditional — no `KillPlayer` call at all, not even a chance
//! roll — whenever the wolves' eat target happens to be Cursed, they
//! become Wolf instead of dying. See
//! the wolves region of `night::resolve_night_sequence`, which handles both the
//! primary wolf-eat target and the Wolf Cub's bonus-kill target (both
//! funnel through the same switch in the real code, Werewolf.cs:3239-3270)
//! before either is turned into a death.

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::Cursed,
    behavior: &Cursed,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoCursed",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 60,
    frozen_loses_action: false,
    deaths: DeathLines::DEFAULT,
    balance: Balance {
        group: BalanceGroup::Village,
        strength,
        pool: Pool::Always,
    },
    dealt: true,
};

/// `GameBalancing.cs::GetStrength`.
fn strength(all_roles: &[Role]) -> i32 {
    1 - all_roles.iter().filter(|&&x| x.is_wolf()).count() as i32 / 2
}

pub struct Cursed;

impl RoleBehavior for Cursed {}
