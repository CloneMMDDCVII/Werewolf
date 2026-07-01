//! Passive — ClumsyGuy has no night action, and casts a lynch vote the
//! same as any other Villager-team role (lynch voting is universal, see
//! `orchestrator::resolve_day`).
//!
//! The real quirk (their own lynch vote has a 50% chance of being
//! redirected to a random player instead of their actual choice,
//! Werewolf.cs:999-1011) is **not modeled**: it's randomized *vote
//! tallying* interference, a different category from anything built so
//! far (the closest relative, Witch/Prince's deterministic overrides,
//! don't involve RNG). Team is still correct.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct ClumsyGuy;

impl RoleBehavior for ClumsyGuy {
    fn team(&self) -> Team {
        Role::ClumsyGuy.team()
    }
}
