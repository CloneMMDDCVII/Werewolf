//! Troublemaker's once-per-game day toggle (Werewolf.cs:965-973): force a
//! second lynch vote today. Same shape as `mayor::Mayor`/
//! `pacifist::Pacifist` — a yes/no via `DayContext::toggle_choice`, gated
//! by `RoleState::primary_used`. Unlike Mayor's `Reveal`, Troublemaker's
//! action *does* interact with another role's here-and-now: it explicitly
//! cancels a same-day Pacifist veto (Werewolf.cs:971: `_pacifistUsed =
//! false; // trouble overrides peace`), which `apply_day_results` applies
//! by dropping `DayAction::Pacify` whenever `DayAction::Trouble` is also
//! present. The actual "run the whole lynch again" mechanic is not
//! modeled — see `DayAction::Trouble`'s doc.

use crate::roles::{DayAction, DayContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Troublemaker;

impl RoleBehavior for Troublemaker {
    fn team(&self) -> Team {
        Role::Troublemaker.team()
    }

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
