//! Blacksmith spreads protective silver over the whole village, once,
//! via a yes/no menu during the day (Werewolf.cs:5083-5092: `SpreadDust`)
//! — not a target pick. An earlier version of this file modeled it as a
//! Gunner-shaped "pick a target" action; that was wrong, caught by
//! reading `SendDayActions` directly rather than assuming the shape from
//! the `AskBlacksmithSilver`-style name. Same once-per-game gate
//! (`RoleState::primary_used`) as before, just triggered by
//! `ctx.toggle_choice` like `sandman::Sandman`/`pacifist::Pacifist`
//! instead of `ctx.chosen_target`.
//!
//! The payoff — every wolf-team role finds no valid targets the
//! *following* night (Werewolf.cs:5191) — isn't this file's job any more
//! than Sandman's "everyone sleeps" payoff is `sandman::Sandman`'s: the
//! wolves are skipped in `game::run_game_with_rules`.

use crate::roles::definition::*;
use crate::roles::{DayAction, DayContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::Blacksmith,
    behavior: &Blacksmith,
    night: NightQuestion::None,
    day: DayQuestion::Ability {
        prompt: Prompt::BlacksmithSilver,
        announce: |blacksmith| Announcement::BlacksmithSpreadSilver { blacksmith },
    },
    info_key: "RoleInfoBlacksmith",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 75,
    frozen_loses_action: false,
    deaths: DeathLines {
        serial_killed: "BlacksmithKilled",
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::RevealedVillage,
        strength: |_| 5,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Blacksmith;

impl RoleBehavior for Blacksmith {
    fn day_action(&self, ctx: &DayContext, state: &mut RoleState) -> Vec<DayAction> {
        if state.primary_used || !ctx.toggle_choice {
            return vec![];
        }
        state.primary_used = true;
        vec![DayAction::SpreadSilver]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::PlayerId;

    fn ctx(toggle_choice: bool) -> DayContext<'static> {
        DayContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: None,
            toggle_choice,
        }
    }

    #[test]
    fn blacksmith_can_spread_silver_once() {
        let bs = Blacksmith;
        let mut state = RoleState::default();
        assert_eq!(bs.day_action(&ctx(true), &mut state), vec![DayAction::SpreadSilver]);
        assert!(state.primary_used);
        assert_eq!(bs.day_action(&ctx(true), &mut state), vec![]);
    }

    #[test]
    fn declining_does_nothing() {
        let bs = Blacksmith;
        let mut state = RoleState::default();
        assert_eq!(bs.day_action(&ctx(false), &mut state), vec![]);
        assert!(!state.primary_used);
    }
}
