//! Passive during the night — Tanner has no night action at all.
//!
//! Tanner's actual win condition (wins the instant they're lynched,
//! overriding everything else — Werewolf.cs:2768-2776) is **already
//! implemented**, just not in this trait: see
//! `game_engine::evaluate_winner_with_kills`, which short-circuits to
//! `Team::Tanner` on any `KillMethod::Lynch` against a `Role::Tanner`
//! victim. Not duplicated here to avoid two sources of truth for the same
//! rule.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct Tanner;

impl RoleBehavior for Tanner {
    fn team(&self) -> Team {
        Role::Tanner.team()
    }
}
