//! Passive — ClumsyGuy has no night action. Their lynch vote has a 50%
//! chance of being redirected to a random living player
//! (Werewolf.cs:999-1011); that is vote-tallying interference, so it lives
//! with the vote in `orchestrator::ask_lynch_vote` rather than here.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct ClumsyGuy;

impl RoleBehavior for ClumsyGuy {
    fn team(&self) -> Team {
        Role::ClumsyGuy.team()
    }
}
