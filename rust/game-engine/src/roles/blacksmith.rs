//! Blacksmith spreads silver over the village once, by day, through a yes/no
//! question (Werewolf.cs:5083-5092). The next night the wolves get no menu and
//! the Snow Wolf no freeze (Werewolf.cs:5191).
//!
//! Resolved in:
//! - the day and night steps of [`game::run_game_with_rules`](crate::game::run_game_with_rules):
//!   recording the silver, then skipping the wolves.

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
