//! Grave Digger has no action of their own — the whole role is a passive
//! trap sprung on whoever *visits* them (Werewolf.cs:2378-2382): a wolf,
//! Harlot, Guardian Angel, Serial Killer, or Thief visiting a Grave Digger
//! falls into the grave and dies instead (`KillMthd.FallGrave`), rather
//! than the visit resolving normally. That's resolution logic belonging to
//! whichever visiting role's action gets applied, not something this
//! passive role's own `RoleBehavior` can express — same "the effect lives
//! on the other side" shape as `wolf_man::WolfMan`'s Seer false-positive.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct GraveDigger;

impl RoleBehavior for GraveDigger {
    fn team(&self) -> Team {
        Role::GraveDigger.team()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grave_digger_is_village_team() {
        assert_eq!(GraveDigger.team(), Team::Village);
    }
}
