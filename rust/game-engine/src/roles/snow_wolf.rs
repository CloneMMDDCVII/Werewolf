//! SnowWolf is wolf-team muscle (`WolfRoles.Concat(SnowWolf)`,
//! Werewolf.cs:1736 onward) and shares the same night eat vote as
//! `wolf::Wolf` — same treatment as `wolf_cub::WolfCub`/`lycan::Lycan`.
//! Team is already correct via `is_wolf_muscle`. No distinguishing quirk
//! of SnowWolf's own is modeled here (e.g. the Guardian Angel's
//! block-detection special case at Werewolf.cs:3110 belongs to
//! `guardian_angel::GuardianAngel`'s resolution, not this file).

use crate::roles::{NightAction, NightContext, PlayerId, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct SnowWolf;

impl RoleBehavior for SnowWolf {
    fn team(&self) -> Team {
        Role::SnowWolf.team()
    }

    fn night_action(&self, ctx: &NightContext, _state: &mut RoleState) -> Vec<NightAction> {
        match ctx.chosen_target {
            Some(target) if is_valid_target(ctx, target) => {
                vec![NightAction::EatVote { target }]
            }
            _ => vec![],
        }
    }
}

fn is_valid_target(ctx: &NightContext, target: PlayerId) -> bool {
    target != ctx.self_id && ctx.alive.contains(&target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snow_wolf_votes_for_a_valid_alive_target() {
        let snow_wolf = SnowWolf;
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
            snow_wolf.night_action(&ctx, &mut state),
            vec![NightAction::EatVote {
                target: PlayerId(2)
            }]
        );
    }
}
