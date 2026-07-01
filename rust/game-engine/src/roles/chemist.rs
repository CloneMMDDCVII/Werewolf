//! Chemist visits a target every night, gambling on a kill
//! (Werewolf.cs:3792-3821): success kills the target (`KillMthd
//! .Chemistry`), failure kills the Chemist instead. Unlike Gunner/Witch,
//! there's no per-game usage limit — the coin flip is the only gate — so
//! this file only validates "is this a living, non-self target," the same
//! restraint `gunner::Gunner` and `witch::Witch` show about not modeling
//! resolution logic that belongs to a future orchestrator.

use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Chemist;

impl RoleBehavior for Chemist {
    fn team(&self) -> Team {
        Role::Chemist.team()
    }

    fn night_action(&self, ctx: &NightContext, _state: &mut RoleState) -> Vec<NightAction> {
        match ctx.chosen_target {
            Some(target) if target != ctx.self_id && ctx.alive.contains(&target) => {
                vec![NightAction::Chemistry { target }]
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
            alive: &[PlayerId(1), PlayerId(2)],
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
    fn chemist_can_gamble_on_a_target_every_night() {
        let chemist = Chemist;
        let mut state = RoleState::default();
        assert_eq!(
            chemist.night_action(&ctx(Some(PlayerId(2))), &mut state),
            vec![NightAction::Chemistry {
                target: PlayerId(2)
            }]
        );
        assert_eq!(
            chemist.night_action(&ctx(Some(PlayerId(2))), &mut state),
            vec![NightAction::Chemistry {
                target: PlayerId(2)
            }],
            "no per-game usage limit, unlike Gunner or Witch"
        );
    }

    #[test]
    fn chemist_cannot_target_itself() {
        let chemist = Chemist;
        let mut state = RoleState::default();
        assert_eq!(chemist.night_action(&ctx(Some(PlayerId(1))), &mut state), vec![]);
    }
}
