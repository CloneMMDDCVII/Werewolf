//! Chef doesn't exist in this project's `Werewolf.cs` at all — confirmed by
//! an exhaustive case-insensitive search of the legacy source, not just an
//! oversight in earlier porting batches. `shared::Role::team`'s doc already
//! flags Chef/Barkeep as verified against a different fork
//! (GreyWolfDev/Werewolf@beta) instead. With no local source to cite line
//! numbers against, this file only asserts the team already established
//! there — no action to model, honestly, rather than invented.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct Chef;

impl RoleBehavior for Chef {
    fn team(&self) -> Team {
        Role::Chef.team()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chef_is_village_team() {
        assert_eq!(Chef.team(), Team::Village);
    }
}
