//! Passive — WiseElder has no action of their own, night or day.
//!
//! The real quirk is entirely reactive to *someone else's* action, and
//! doesn't save the Wise Elder — they still die from a Gunner's shot like
//! anyone else (checked directly: `KillPlayer` runs unconditionally right
//! after the Wise Elder switch case, Werewolf.cs:2895). What actually
//! happens is punitive: the *Gunner* gets demoted to Villager
//! (Werewolf.cs:2887-2889: `Transform(gunner, IRole.Villager, ...)`).
//! That's resolution logic belonging to Gunner's shot, not to this
//! passive file — see `game::demote_gunner_if_shot_a_wise_elder`, which
//! `run_game` applies before the shot itself is turned into a death.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct WiseElder;

impl RoleBehavior for WiseElder {
    fn team(&self) -> Team {
        Role::WiseElder.team()
    }
}
