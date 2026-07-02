//! Grave Digger has no action of their own — the whole role is a passive
//! trap sprung on whoever *visits* them (Werewolf.cs:2377-2411): a wolf,
//! Harlot, or Guardian Angel visiting a Grave Digger who's dug at least
//! one grave risks falling in and dying instead (`KillMthd.FallGrave`),
//! rather than the visit resolving normally. That's resolution logic
//! belonging to whichever visiting role's action gets applied, not
//! something this passive role's own `RoleBehavior` can express — same
//! "the effect lives on the other side" shape as `wolf_man::WolfMan`'s
//! Seer false-positive. See `orchestrator::resolve_grave_digger_falls`
//! (Harlot/Guardian Angel — the two cases with a single, nameable
//! visitor) and `game::run_game`'s `dug_graves` tracking, which counts
//! deaths since the last time this role's grave count was computed
//! (Werewolf.cs:5294-5299) the same way every night, whether or not this
//! role is even alive to look. A Serial Killer visiting a Grave Digger
//! never falls at all (Werewolf.cs:2382-2387 — it's the one case in the
//! whole trap that isn't RNG, so it isn't a gap here). Wolves visiting a
//! Grave Digger *can* fall too, but the real code attributes the fall to
//! one specific wolf picked at random out of the voting pack
//! (`voteWolves.ElementAt(...)`) — this proof-of-concept's wolf-eat vote
//! only tracks a majority target, not individual visitor identity, so
//! that case is left out rather than guessed at.

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
