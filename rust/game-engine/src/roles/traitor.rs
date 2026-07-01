//! Passive during the night — Traitor has no night action of its own.
//!
//! The real Traitor mechanic (turns Wolf if the last living Wolf dies) is
//! **not modeled here**. In the legacy code it lives at the very start of
//! `CheckForWin`, before the win-check switch statement even runs
//! (Werewolf.cs:4499-4512) — it's a transform triggered by *another
//! player's* death, not by anything Traitor itself does on its own turn.
//! That needs an `on_other_player_death` style hook this trait doesn't
//! have yet (same category of gap as `cursed::Cursed`'s bite-transform).
//! Team is still correct in the meantime.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct Traitor;

impl RoleBehavior for Traitor {
    fn team(&self) -> Team {
        Role::Traitor.team()
    }
}
