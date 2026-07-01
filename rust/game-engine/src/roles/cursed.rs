//! Passive during the night — Cursed has no night action of its own.
//!
//! The real Cursed mechanic (turns Wolf if bitten by a wolf) is **not
//! modeled here**. Like `traitor::Traitor`, it's a transform triggered by
//! being on the receiving end of another player's night action, not
//! something Cursed decides for itself — needs an `on_bitten`/general
//! transform hook this trait doesn't have yet. Team is still correct in
//! the meantime.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct Cursed;

impl RoleBehavior for Cursed {
    fn team(&self) -> Team {
        Role::Cursed.team()
    }
}
