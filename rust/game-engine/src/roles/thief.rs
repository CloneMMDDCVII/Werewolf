//! Thief steals a target's role, night 1 only, in the base (non-`ThiefFull`)
//! mode this proof-of-concept models (Werewolf.cs:4136-4163: `if (GameDay
//! == 1)`). Same one-shot shape as `doppelganger::Doppelganger`
//! (`RoleState::primary_used`), but the payoff is immediate rather than
//! waiting on a death — see `NightAction::StealRole`'s doc for why that
//! earned its own action variant instead of reusing `ChooseRoleModel`.
//!
//! In `ThiefFull` mode (`GameRules::thief_full`) `run_game` clears
//! `primary_used` every night so the Thief can try again, and resolves the
//! steal at the end of the night through the Thief region of `night::resolve_night_sequence`
//! instead (Werewolf.cs:4164-4189). The swap itself is
//! `game::swap_roles` in both modes.

use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Thief;

impl RoleBehavior for Thief {
    fn team(&self) -> Team {
        Role::Thief.team()
    }

    fn night_action(&self, ctx: &NightContext, state: &mut RoleState) -> Vec<NightAction> {
        if state.primary_used {
            return vec![];
        }
        match ctx.chosen_target {
            Some(target) if target != ctx.self_id && ctx.alive.contains(&target) => {
                state.primary_used = true;
                vec![NightAction::StealRole {
                    thief: ctx.self_id,
                    target,
                }]
            }
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::PlayerId;

    fn ctx(target: Option<PlayerId>) -> NightContext<'static> {
        NightContext {
            alive: &[PlayerId(1), PlayerId(2), PlayerId(3)],
            self_id: PlayerId(1),
            chosen_target: target,
            heal_target: None,
            poison_target: None,
            love_targets: None,
            wolf_target: None,
            toggle_choice: false,
        }
    }

    #[test]
    fn thief_steals_a_role_once() {
        let thief = Thief;
        let mut state = RoleState::default();
        assert_eq!(
            thief.night_action(&ctx(Some(PlayerId(2))), &mut state),
            vec![NightAction::StealRole {
                thief: PlayerId(1),
                target: PlayerId(2)
            }]
        );
        assert!(state.primary_used);
        assert_eq!(thief.night_action(&ctx(Some(PlayerId(3))), &mut state), vec![]);
    }

    #[test]
    fn thief_cannot_steal_from_itself() {
        let thief = Thief;
        let mut state = RoleState::default();
        assert_eq!(thief.night_action(&ctx(Some(PlayerId(1))), &mut state), vec![]);
    }
}
