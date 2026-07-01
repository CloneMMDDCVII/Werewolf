//! Cupid links two players as lovers, once, typically on the first night.
//! The link itself doesn't live here — see `lovers::LoversState` — because
//! "being in love" is state about *two other players*, not about Cupid.
//! This file is deliberately small: Cupid's whole job is producing one
//! `LinkLovers` action; everything about what loving someone *means* for
//! win conditions belongs to `lovers`, not to Cupid.

use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Cupid;

impl RoleBehavior for Cupid {
    fn team(&self) -> Team {
        Role::Cupid.team()
    }

    fn night_action(&self, ctx: &NightContext, state: &mut RoleState) -> Vec<NightAction> {
        if state.primary_used {
            return vec![];
        }
        match ctx.love_targets {
            Some((a, b)) if a != b && ctx.alive.contains(&a) && ctx.alive.contains(&b) => {
                state.primary_used = true;
                vec![NightAction::LinkLovers { a, b }]
            }
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::PlayerId;

    #[test]
    fn cupid_links_two_distinct_alive_players_once() {
        let cupid = Cupid;
        let ctx = NightContext {
            alive: &[PlayerId(1), PlayerId(2), PlayerId(3)],
            self_id: PlayerId(1),
            chosen_target: None,
            heal_target: None,
            poison_target: None,
            love_targets: Some((PlayerId(2), PlayerId(3))),
        };
        let mut state = RoleState::default();
        assert_eq!(
            cupid.night_action(&ctx, &mut state),
            vec![NightAction::LinkLovers {
                a: PlayerId(2),
                b: PlayerId(3)
            }]
        );
        // Second attempt this game: already used.
        assert_eq!(cupid.night_action(&ctx, &mut state), vec![]);
    }

    #[test]
    fn cupid_cannot_link_a_player_to_themselves() {
        let cupid = Cupid;
        let ctx = NightContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: None,
            heal_target: None,
            poison_target: None,
            love_targets: Some((PlayerId(2), PlayerId(2))),
        };
        let mut state = RoleState::default();
        assert_eq!(cupid.night_action(&ctx, &mut state), vec![]);
    }
}
