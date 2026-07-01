//! Passive — Prince has no action of their own, day or night. The real
//! ability is entirely reactive: the *first* time Prince is lynched, they
//! survive instead of dying (Werewolf.cs:2745-2751: `!lynched.HasUsedAbility`
//! guards the kill). That's not something Prince decides via `day_action`
//! — it's `orchestrator::apply_day_results` checking, at the moment the
//! lynch would apply, whether the target is a Prince who hasn't spent
//! their immunity yet (`RoleState::primary_used`), same "one-shot survival"
//! shape as Witch's heal canceling the wolf kill.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct Prince;

impl RoleBehavior for Prince {
    fn team(&self) -> Team {
        Role::Prince.team()
    }
}
