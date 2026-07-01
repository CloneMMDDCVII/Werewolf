//! No night action — Gunner's ability (two bullets, shoot a suspected
//! player) is a **day-phase** action in the legacy game (Werewolf.cs:
//! 2871-2896, resolved alongside day processing, not the night loop).
//! This trait only has a `night_action` hook so far; deliberately not
//! forcing Gunner's day ability through it just to have *something* here —
//! it needs its own `day_action` hook once the day-phase loop gets ported.
//! Team is correct in the meantime.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct Gunner;

impl RoleBehavior for Gunner {
    fn team(&self) -> Team {
        Role::Gunner.team()
    }
}
