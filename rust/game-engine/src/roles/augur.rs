//! Augur has no choice to make at all (Werewolf.cs:4044-4060): every night
//! they're automatically told one role that is *not* in the game (picked
//! at random from roles not yet seen and not held by any surviving
//! player), or told nothing if every role has already been revealed to
//! them. There's no target, no toggle — nothing for a `NightContext` to
//! carry — so this role is passive from `RoleBehavior`'s perspective; the
//! actual "which role to reveal" bookkeeping (`augur.SawRoles`) is
//! presentation logic for a future orchestrator, not a decision this file
//! could validate even in principle.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct Augur;

impl RoleBehavior for Augur {
    fn team(&self) -> Team {
        Role::Augur.team()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn augur_is_village_team() {
        assert_eq!(Augur.team(), Team::Village);
    }
}
