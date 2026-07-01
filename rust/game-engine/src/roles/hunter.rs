//! Passive during the night — Hunter has no night action of their own; the
//! whole ability fires only at death time, so there's nothing for
//! `RoleBehavior` to express here.
//!
//! The real Hunter ability — a revenge shot the moment they die, by any
//! method (Werewolf.cs:5420-5484: `HunterFinalShot`) — is modeled in
//! `orchestrator::resolve_hunter_shots`, not here: it needs asking the
//! *just-died* Hunter one more question before the round finishes, which
//! is a different shape of hook than `RoleBehavior` provides (that trait
//! only ever answers "what does a *living* player with this role do"),
//! so it's orchestrator-level machinery instead, called from `run_game`
//! right after a batch of deaths resolves.
//!
//! **Scoped to the core mechanic** — see `resolve_hunter_shots`'s own doc
//! for the specific legacy exceptions (Grave Digger's trap, Arsonist's
//! burn, the wolf-standoff sub-case, a couple of 2-player endgame
//! shortcuts, and "Domino" chaining) this proof-of-concept doesn't
//! distinguish yet.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct Hunter;

impl RoleBehavior for Hunter {
    fn team(&self) -> Team {
        Role::Hunter.team()
    }
}
