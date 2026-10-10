//! Sandman chooses by day, once per game, whether to put everyone to
//! sleep (Werewolf.cs:950-961, asked at 5070-5080) — a yes/no toggle, not
//! a target pick, hence `DayContext::toggle_choice`.
//!
//! The following night is skipped outright: nobody is asked anything and
//! nothing resolves (Werewolf.cs:3011-3020). `game::run_game` does that
//! when it sees `DayAction::Sleep`.

use crate::roles::definition::*;
use crate::roles::{DayAction, DayContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::Sandman,
    behavior: &Sandman,
    night: NightQuestion::None,
    day: DayQuestion::Ability {
        prompt: Prompt::SandmanSleep,
        announce: |sandman| Announcement::SandmanSleepAll { sandman },
    },
    info_key: "RoleInfoSandman",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 60,
    frozen_loses_action: false,
    deaths: DeathLines::DEFAULT,
    balance: Balance {
        group: BalanceGroup::RevealedVillage,
        strength: |_| 3,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Sandman;

impl RoleBehavior for Sandman {
    fn day_action(&self, ctx: &DayContext, state: &mut RoleState) -> Vec<DayAction> {
        if state.primary_used || !ctx.toggle_choice {
            return vec![];
        }
        state.primary_used = true;
        vec![DayAction::Sleep]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::PlayerId;

    fn ctx(toggle_choice: bool) -> DayContext<'static> {
        DayContext {
            alive: &[PlayerId(1), PlayerId(2)][..],
            self_id: PlayerId(1),
            chosen_target: None,
            toggle_choice,
        }
    }

    #[test]
    fn sandman_can_put_everyone_to_sleep_once() {
        let sandman = Sandman;
        let mut state = RoleState::default();
        assert_eq!(sandman.day_action(&ctx(true), &mut state), vec![DayAction::Sleep]);
        assert!(state.primary_used);
        // Already used - even with toggle_choice true again, no repeat.
        assert_eq!(sandman.day_action(&ctx(true), &mut state), vec![]);
    }

    #[test]
    fn sandman_does_nothing_if_they_decline() {
        let sandman = Sandman;
        let mut state = RoleState::default();
        assert_eq!(sandman.day_action(&ctx(false), &mut state), vec![]);
        assert!(!state.primary_used);
    }
}
