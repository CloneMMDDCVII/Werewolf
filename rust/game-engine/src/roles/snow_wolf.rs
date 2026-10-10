//! SnowWolf is wolf-team muscle for win-condition purposes (`is_wolf_muscle`
//! counts it), but it does **not** share the main wolf-eat vote: the
//! legacy vote collection uses `WolfRoles = { Wolf, AlphaWolf, WolfCub,
//! Lycan }` (Werewolf.cs:48), which leaves it out. Its night ability is a
//! freeze instead, asked and resolved by the `freeze` module (it needs the
//! previous night's target and the whole night's actions, neither of
//! which a `NightContext` carries), so this file stays passive. When no
//! other wolf is left it turns into a plain Wolf (`game::apply_transforms`).

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct SnowWolf;

impl RoleBehavior for SnowWolf {
    fn team(&self) -> Team {
        Role::SnowWolf.team()
    }
}
