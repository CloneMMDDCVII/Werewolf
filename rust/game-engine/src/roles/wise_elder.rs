//! Passive — WiseElder has no action of their own, night or day.
//!
//! The real quirk is entirely reactive to *someone else's* action: if shot
//! by Gunner, the Gunner's bullet reverts to Villager instead of the
//! WiseElder dying (Werewolf.cs:2887-2889: `Transform(gunner,
//! IRole.Villager, ...)`). That's resolution logic belonging to Gunner's
//! shot, not to this file — `orchestrator::apply_day_results` doesn't
//! attempt it (same "not modeled" category as the rest of Gunner's shot
//! resolution). Team is still correct.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct WiseElder;

impl RoleBehavior for WiseElder {
    fn team(&self) -> Team {
        Role::WiseElder.team()
    }
}
