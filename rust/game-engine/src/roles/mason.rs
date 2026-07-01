//! Passive during the night — Masons have no night action. The real
//! ability (knowing who the other Masons are, Werewolf.cs:1660-1664) is
//! purely informational, same category as `beholder::Beholder`. Nothing
//! to model beyond `team()`.

use crate::roles::RoleBehavior;
use shared::{Role, Team};

pub struct Mason;

impl RoleBehavior for Mason {
    fn team(&self) -> Team {
        Role::Mason.team()
    }
}
