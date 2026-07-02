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
//! `game::resolve_wolf_eat_target`, which `run_game` calls for both the
//! primary wolf-eat target and the Wolf Cub's bonus-kill target (both
//! funnel through the same switch in the real code, Werewolf.cs:3239-3270)
//! before either is turned into a death.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct Cursed;

impl RoleBehavior for Cursed {
    fn team(&self) -> Team {
        Role::Cursed.team()
    }
}
