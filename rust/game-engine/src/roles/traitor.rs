//! Passive during the night — Traitor has no night action of its own.
//!
//! The real Traitor mechanic (turns Wolf if the last living Wolf dies,
//! Werewolf.cs:4499-4512, at the very start of `CheckForWin`, before the
//! win-check switch statement even runs) **is modeled**, but not in this
//! file: it's a transform triggered by the *alive roster's* composition,
//! not anything Traitor does on its own turn — see
//! `game::apply_transforms`, which runs this check every round rather than
//! reacting to a specific death event (simpler and equivalent: "no wolf
//! muscle currently alive" is the same condition either way). Team here is
//! still just the base (pre-transform) `Role::Traitor.team()`.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct Traitor;

impl RoleBehavior for Traitor {
    fn team(&self) -> Team {
        Role::Traitor.team()
    }
}
