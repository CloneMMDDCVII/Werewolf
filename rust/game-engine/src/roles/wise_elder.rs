//! Passive — WiseElder has no action of their own, night or day. Both real
//! quirks are entirely reactive to *someone else's* action, so neither
//! lives in this file:
//!
//! - A Gunner's shot doesn't save them — they still die like anyone else
//!   (checked directly: `KillPlayer` runs unconditionally right after the
//!   Wise Elder switch case, Werewolf.cs:2895). What actually happens is
//!   punitive: the *Gunner* gets demoted to Villager
//!   (Werewolf.cs:2887-2889: `Transform(gunner, IRole.Villager, ...)`).
//!   See `game::demote_gunner_if_shot_a_wise_elder`, applied before the
//!   shot itself is turned into a death.
//! - A wolf-eat *does* save them, but only once: the first non-bitten
//!   wolf-eat attempt has no `KillPlayer` call at all
//!   (Werewolf.cs:3392-3406, guarded by `target.HasUsedAbility`) — the
//!   wolves are told they failed and the Wise Elder is warned, then a
//!   *second* attempt falls through to a normal kill (`goto default`).
//!   See `game::resolve_wolf_eat_target`, which tracks this with the same
//!   `RoleState::primary_used` one-shot flag every other "first time
//!   only" role uses.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct WiseElder;

impl RoleBehavior for WiseElder {
    fn team(&self) -> Team {
        Role::WiseElder.team()
    }
}
