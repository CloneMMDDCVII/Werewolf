//! Sorcerer checks one player overnight, specifically hunting for the Seer
//! (Werewolf.cs:3958-3975) — same action shape as Detective/Fool
//! (validate a target, emit `Investigate`), just a different question
//! being asked. What information comes back is resolution logic this
//! file doesn't attempt, same as the other investigate-shaped roles.

use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Sorcerer;

impl RoleBehavior for Sorcerer {
    fn team(&self) -> Team {
        Role::Sorcerer.team()
    }

    fn night_action(&self, ctx: &NightContext, _state: &mut RoleState) -> Vec<NightAction> {
        match ctx.chosen_target {
            Some(target) if target != ctx.self_id && ctx.alive.contains(&target) => {
                vec![NightAction::Investigate { target }]
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
    fn sorcerer_investigates_a_valid_target() {
        let sorcerer = Sorcerer;
        let ctx = NightContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: Some(PlayerId(2)),
            heal_target: None,
            poison_target: None,
            love_targets: None,
            wolf_target: None,
            toggle_choice: false,
        };
        let mut state = RoleState::default();
        assert_eq!(
            sorcerer.night_action(&ctx, &mut state),
            vec![NightAction::Investigate {
                target: PlayerId(2)
            }]
        );
    }
}
