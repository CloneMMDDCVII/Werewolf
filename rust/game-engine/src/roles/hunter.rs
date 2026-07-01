//! Passive during the night — Hunter has no night action of their own.
//!
//! The real Hunter ability (a revenge shot when Hunter dies,
//! Werewolf.cs:3325-3335 for the wolf-standoff case, plus the general
//! death-triggered version) is **not modeled here**: it needs asking the
//! *just-died* Hunter one more question before the round finishes, which
//! is a different shape of hook than anything built so far —
//! `apply_night_results`/`apply_day_results` only ever turn already-made
//! decisions into deaths, they don't turn around and ask a new one
//! afterward. A real implementation needs the orchestrator to support
//! "a death can trigger one more question," which doesn't exist yet.
//! Team is still correct in the meantime.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct Hunter;

impl RoleBehavior for Hunter {
    fn team(&self) -> Team {
        Role::Hunter.team()
    }
}
