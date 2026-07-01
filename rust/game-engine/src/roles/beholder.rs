//! Passive during the night — Beholder has no night action. The real
//! ability (silently learns who the Seer is, Werewolf.cs:2055-2110) is
//! purely informational: it never produces a `NightAction` for anyone else
//! to react to, just a private message. Nothing to model beyond `team()`.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct Beholder;

impl RoleBehavior for Beholder {
    fn team(&self) -> Team {
        Role::Beholder.team()
    }
}
