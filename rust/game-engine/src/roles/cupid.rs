//! Cupid links two players as lovers, once, on the first night.
//!
//! Resolved in:
//! - the night step of [`game::run_game_with_rules`](crate::game::run_game_with_rules): recording the link;
//! - [`lovers`](crate::roles::lovers): what being in love means.

use crate::roles::definition::*;
use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::Cupid,
    behavior: &Cupid,
    night: NightQuestion::Pair(Prompt::CupidLink),
    day: DayQuestion::None,
    info_key: "RoleInfoCupid",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines {
        serial_killed: "CupidKilled",
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 2,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Cupid;

impl RoleBehavior for Cupid {
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
            wolf_target: None,
            toggle_choice: false,
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
            wolf_target: None,
            toggle_choice: false,
        };
        let mut state = RoleState::default();
        assert_eq!(cupid.night_action(&ctx, &mut state), vec![]);
    }
}
