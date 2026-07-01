//! Passive role, no night action — mechanically identical to `Villager`.
//! In the legacy game the only "Drunk" behavior is a display-layer joke
//! (shown a wrong role hint), which affects messaging, not game state, so
//! there's nothing to port here beyond `team()`.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct Drunk;

impl RoleBehavior for Drunk {
    fn team(&self) -> Team {
        Role::Drunk.team()
    }
}
