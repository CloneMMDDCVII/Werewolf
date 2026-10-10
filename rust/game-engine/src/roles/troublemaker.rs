//! Troublemaker may force a second lynch today, once (Werewolf.cs:965-973). A
//! Pacifist on the same day cancels it; the last one to answer wins.
//!
//! Resolved in:
//! - [`orchestrator::last_day_veto`](crate::orchestrator::last_day_veto): which veto stands;
//! - [`game::run_game_with_rules`](crate::game::run_game_with_rules): the second lynch.

use crate::roles::definition::*;
use crate::roles::{DayAction, DayContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::Troublemaker,
    behavior: &Troublemaker,
    night: NightQuestion::None,
    day: DayQuestion::Ability {
        prompt: Prompt::TroublemakerTrouble,
        announce: |troublemaker| Announcement::TroublemakerDoubleLynch { troublemaker },
    },
    info_key: "RoleInfoTroublemaker",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines::DEFAULT,
    balance: Balance {
        group: BalanceGroup::RevealedVillage,
        strength: |_| 5,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Troublemaker;

impl RoleBehavior for Troublemaker {
    fn day_action(&self, ctx: &DayContext, state: &mut RoleState) -> Vec<DayAction> {
        if state.primary_used || !ctx.toggle_choice {
            return vec![];
        }
        state.primary_used = true;
        vec![DayAction::Trouble]
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
    fn troublemaker_can_stir_trouble_once() {
        let tm = Troublemaker;
        let mut state = RoleState::default();
        assert_eq!(tm.day_action(&ctx(true), &mut state), vec![DayAction::Trouble]);
        assert!(state.primary_used);
        assert_eq!(tm.day_action(&ctx(true), &mut state), vec![]);
    }

    #[test]
    fn troublemaker_declining_does_nothing() {
        let tm = Troublemaker;
        let mut state = RoleState::default();
        assert_eq!(tm.day_action(&ctx(false), &mut state), vec![]);
        assert!(!state.primary_used);
    }
}
