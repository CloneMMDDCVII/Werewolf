//! Serial Killer kills a target overnight, personally and directly
//! (Werewolf.cs:2340-2348, 2380 onward) — normally the only one of their
//! kind, so unlike `wolf::Wolf`'s `EatVote` there's no consensus to tally.
//! `apply_night_results` applies this as an unconditional kill.

use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct SerialKiller;

impl RoleBehavior for SerialKiller {
    fn team(&self) -> Team {
        Role::SerialKiller.team()
    }

    fn night_action(&self, ctx: &NightContext, _state: &mut RoleState) -> Vec<NightAction> {
        match ctx.chosen_target {
            Some(target) if target != ctx.self_id && ctx.alive.contains(&target) => {
                vec![NightAction::SerialKillVote { target }]
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
    fn serial_killer_kills_a_valid_target() {
        let sk = SerialKiller;
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
            sk.night_action(&ctx, &mut state),
            vec![NightAction::SerialKillVote {
                target: PlayerId(2)
            }]
        );
    }

    #[test]
    fn serial_killer_cannot_target_itself() {
        let sk = SerialKiller;
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
        assert_eq!(sk.night_action(&ctx, &mut state), vec![]);
    }
}
