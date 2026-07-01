//! Harlot visits one player overnight (Werewolf.cs:3172-3175, `AskVisit`).
//! Visiting is allowed on herself (staying home) as well as any other
//! living player.
//!
//! Dying if the target turns out to be the wolves' actual victim that
//! same night **is** modeled (Werewolf.cs:2358-2360) — see
//! `orchestrator::resolve_harlot_visit_deaths`, which needs `self_id` on
//! the `Visit` action to know who to kill. Being unable to visit someone
//! already occupied (Werewolf.cs:2424-2428) is resolution logic across
//! multiple players' actions this proof-of-concept still doesn't attempt.

use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Harlot;

impl RoleBehavior for Harlot {
    fn team(&self) -> Team {
        Role::Harlot.team()
    }

    fn night_action(&self, ctx: &NightContext, _state: &mut RoleState) -> Vec<NightAction> {
        match ctx.chosen_target {
            Some(target) if ctx.alive.contains(&target) => {
                vec![NightAction::Visit {
                    visitor: ctx.self_id,
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

    #[test]
    fn harlot_may_visit_herself_to_stay_home() {
        let harlot = Harlot;
        let ctx = NightContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: Some(PlayerId(1)),
            heal_target: None,
            poison_target: None,
            love_targets: None,
            wolf_target: None,
            toggle_choice: false,
        };
        let mut state = RoleState::default();
        assert_eq!(
            harlot.night_action(&ctx, &mut state),
            vec![NightAction::Visit {
                visitor: PlayerId(1),
                target: PlayerId(1)
            }]
        );
    }

    #[test]
    fn harlot_cannot_visit_a_dead_player() {
        let harlot = Harlot;
        let ctx = NightContext {
            alive: &[PlayerId(1)],
            self_id: PlayerId(1),
            chosen_target: Some(PlayerId(2)),
            heal_target: None,
            poison_target: None,
            love_targets: None,
            wolf_target: None,
            toggle_choice: false,
        };
        let mut state = RoleState::default();
        assert_eq!(harlot.night_action(&ctx, &mut state), vec![]);
    }
}
