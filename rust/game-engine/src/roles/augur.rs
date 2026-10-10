//! Augur has no choice to make at all (Werewolf.cs:4044-4060): every night
//! they're automatically told one role that is *not* in the game (picked
//! at random from the game's role pool, among roles not yet shown and not
//! held by any surviving player), or told nothing new once every such role
//! has been shown. There's no target and no toggle, so this role is
//! passive from `RoleBehavior`'s perspective; the reading and the
//! `SawRoles` memory live in `reveals::resolve_night_readings`.

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
