//! Cultist proposes a conversion target overnight (Werewolf.cs:3690-3735
//! onward). Real conversion is per-target-role RNG resolution ("convert
//! Blacksmith at `Settings.BlacksmithConversionChance`", etc.) this file
//! doesn't attempt — same "validate a target, emit the proposal" shape as
//! Wolf's `EatVote`, just for `ConvertVote` instead. Unlike Wolf, there's
//! no `NightFact` dependency here: the cult doesn't need anyone else's
//! decision resolved first.

use crate::roles::{NightAction, NightContext, PlayerId, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Cultist;

impl RoleBehavior for Cultist {
    fn team(&self) -> Team {
        Role::Cultist.team()
    }

    fn night_action(&self, ctx: &NightContext, _state: &mut RoleState) -> Vec<NightAction> {
        match ctx.chosen_target {
            Some(target) if is_valid_target(ctx, target) => {
                vec![NightAction::ConvertVote { target }]
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
    fn cultist_proposes_a_valid_conversion_target() {
        let cultist = Cultist;
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
            cultist.night_action(&ctx, &mut state),
            vec![NightAction::ConvertVote {
                target: PlayerId(2)
            }]
        );
    }

    #[test]
    fn cultist_cannot_convert_itself() {
        let cultist = Cultist;
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
        assert_eq!(cultist.night_action(&ctx, &mut state), vec![]);
    }
}
