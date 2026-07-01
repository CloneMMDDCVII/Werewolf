//! Lovers is not a role — it's a relationship state any two players can end
//! up in via Cupid's `LinkLovers` action. It's modeled separately from
//! `RoleBehavior` on purpose: "one role, one file" doesn't fit something
//! that's about a *pair* of players regardless of what roles they hold
//! (per the legacy win-check, even a Wolf and a Villager can be lovers —
//! Werewolf.cs:4654 explicitly handles that "forbidden love" case).
//!
//! This also closes a gap flagged earlier in `sim`'s fixture-replay report:
//! Lovers outcomes couldn't be verified there because the SQL export never
//! captured `InLove` pairing data. This is the model that data would need
//! to flow into once the export is extended.

use crate::roles::PlayerId;
use shared::Team;

#[derive(Debug, Clone, Default)]
pub struct LoversState {
    pairs: Vec<(PlayerId, PlayerId)>,
}

impl LoversState {
    pub fn link(&mut self, a: PlayerId, b: PlayerId) {
        self.pairs.push((a, b));
    }

    pub fn partner_of(&self, id: PlayerId) -> Option<PlayerId> {
        self.pairs.iter().find_map(|&(a, b)| {
            if a == id {
                Some(b)
            } else if b == id {
                Some(a)
            } else {
                None
            }
        })
    }

    /// Mirrors Werewolf.cs:4526-4527: exactly two players left alive, and
    /// they're mutually in love — Lovers win, overriding whatever their
    /// individual teams would otherwise say.
    pub fn is_lovers_win(&self, alive: &[PlayerId]) -> bool {
        alive.len() == 2 && self.partner_of(alive[0]) == Some(alive[1])
    }
}

pub const LOVERS_WIN_TEAM: Team = Team::Lovers;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_linked_survivors_win_as_lovers() {
        let mut lovers = LoversState::default();
        lovers.link(PlayerId(1), PlayerId(2));
        assert!(lovers.is_lovers_win(&[PlayerId(1), PlayerId(2)]));
        assert!(lovers.is_lovers_win(&[PlayerId(2), PlayerId(1)]));
    }

    #[test]
    fn unlinked_pair_does_not_win_as_lovers() {
        let lovers = LoversState::default();
        assert!(!lovers.is_lovers_win(&[PlayerId(1), PlayerId(2)]));
    }

    #[test]
    fn a_third_survivor_rules_out_lovers_win_even_if_linked() {
        let mut lovers = LoversState::default();
        lovers.link(PlayerId(1), PlayerId(2));
        assert!(!lovers.is_lovers_win(&[PlayerId(1), PlayerId(2), PlayerId(3)]));
    }
}
