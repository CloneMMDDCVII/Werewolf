//! The template for a role that does nothing special. If you're adding a
//! new role and it has no night action, copy this file: rename the type,
//! keep `team()`, delete nothing else. `night_action` is left at its
//! default (no actions) rather than overridden here on purpose, so this
//! stays the shortest possible example of "a role that just exists."

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct Villager;

impl RoleBehavior for Villager {
    fn team(&self) -> Team {
        Role::Villager.team()
    }
}
