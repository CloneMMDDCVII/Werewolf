//! Cultist Hunter checks one player overnight, hunting specifically for
//! Cultists (Werewolf.cs:3559 onward) — same shape as Detective/Fool/
//! Sorcerer. What information comes back, and the special Werewolf.cs:
//! 4576-4580 case (killing the last Cultist outright if that's who's
//! left) are resolution logic this file doesn't attempt.

use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct CultistHunter;

impl RoleBehavior for CultistHunter {
    fn team(&self) -> Team {
        Role::CultistHunter.team()
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
    fn cultist_hunter_investigates_a_valid_target() {
        let ch = CultistHunter;
        let ctx = NightContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: Some(PlayerId(2)),
            heal_target: None,
            poison_target: None,
            love_targets: None,
            wolf_target: None,
        };
        let mut state = RoleState::default();
        assert_eq!(
            ch.night_action(&ctx, &mut state),
            vec![NightAction::Investigate {
                target: PlayerId(2)
            }]
        );
    }
}
