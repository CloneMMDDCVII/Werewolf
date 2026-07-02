//! SnowWolf is wolf-team muscle for win-condition purposes (`is_wolf_muscle`
//! counts it, and its own night-1 conversion excludes it from stealable/
//! convertible pools alongside `WolfRoles`), but it does **not** share the
//! main wolf-eat vote. Confirmed directly: the real vote-collection code
//! (Werewolf.cs:48, `WolfRoles = { Wolf, AlphaWolf, WolfCub, Lycan }`, and
//! the "Wolf Night - Non-snow wolves" region's
//! `nightPlayers.GetPlayersForRoles(WolfRoles)`) explicitly excludes
//! `IRole.SnowWolf` — the name is literal, it has its own separate "Snow
//! Wolf Night" region with its own ability (freezing a player so their
//! *next* night's action fails, Werewolf.cs's Snow Wolf Night region and
//! `VisitResult`'s `.Frozen` check), not a vote in the shared eat.
//!
//! An earlier version of this file gave SnowWolf the exact same
//! `EatVote` night action as `wolf::Wolf`, which was wrong — caught by
//! checking the source's own `WolfRoles` array rather than assuming
//! "wolf team" implies "votes in the wolf eat." The real Freeze ability
//! itself still isn't modeled here (it would need a generic "this
//! player's next action is nulled" mechanism this trait doesn't have),
//! so this file is honestly passive in the meantime, same treatment as
//! `cursed::Cursed`/`traitor::Traitor` before their own reactive quirks
//! got modeled.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct SnowWolf;

impl RoleBehavior for SnowWolf {
    fn team(&self) -> Team {
        Role::SnowWolf.team()
    }
}
