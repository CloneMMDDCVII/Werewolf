//! Mayor may reveal their role once, by day (Werewolf.cs:899-908). After that
//! their lynch vote counts twice (Werewolf.cs:2649-2652).
//!
//! Resolved in:
//! - [`orchestrator::day::resolve_day`](crate::orchestrator::day::resolve_day) and
//!   [`orchestrator::day::resolve_lynch_vote`](crate::orchestrator::day::resolve_lynch_vote)
//!   (a Troublemaker's second lynch): the double vote.

use crate::roles::definition::*;
use crate::roles::{DayAction, DayContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::Mayor,
    behavior: &Mayor,
    night: NightQuestion::None,
    day: DayQuestion::Ability {
        prompt: Prompt::MayorReveal,
        announce: |mayor| Announcement::MayorReveal { mayor },
    },
    info_key: "RoleInfoMayor",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines {
        serial_killed: "MayorKilled",
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::RevealedVillage,
        strength: |_| 4,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Mayor;

impl RoleBehavior for Mayor {
    fn day_action(&self, ctx: &DayContext, state: &mut RoleState) -> Vec<DayAction> {
        if state.primary_used || !ctx.toggle_choice {
            return vec![];
        }
        state.primary_used = true;
        vec![DayAction::Reveal]
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
    fn mayor_can_reveal_once() {
        let mayor = Mayor;
        let mut state = RoleState::default();
        assert_eq!(mayor.day_action(&ctx(true), &mut state), vec![DayAction::Reveal]);
        assert_eq!(mayor.day_action(&ctx(true), &mut state), vec![]);
    }

    #[test]
    fn mayor_stays_hidden_if_they_decline() {
        let mayor = Mayor;
        let mut state = RoleState::default();
        assert_eq!(mayor.day_action(&ctx(false), &mut state), vec![]);
    }
}
