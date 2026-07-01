//! Barkeep, like `chef::Chef`, doesn't exist in this project's
//! `Werewolf.cs` — same beta-fork-only status, same honest "no source, no
//! invented mechanic" treatment.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct Barkeep;

impl RoleBehavior for Barkeep {
    fn team(&self) -> Team {
        Role::Barkeep.team()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn barkeep_is_village_team() {
        assert_eq!(Barkeep.team(), Team::Village);
    }
}
