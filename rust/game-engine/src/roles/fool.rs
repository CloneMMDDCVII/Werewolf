//! Fool investigates one player overnight, same action shape as Detective
//! (Werewolf.cs:5174-5179, grouped with Seer/Sorcerer/Oracle under
//! `AskSee`) — but the answer the Fool gets back is fake: the legacy code
//! shows a *random* role from the remaining player pool, masking any wolf
//! variant as generically "Wolf" (Werewolf.cs:3985-4000). That
//! randomization is a display-layer detail for a future orchestrator to
//! produce, not something this file decides — it only validates the
//! target, same as Detective.

use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Fool;

impl RoleBehavior for Fool {
    fn team(&self) -> Team {
        Role::Fool.team()
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
    fn fool_investigates_a_valid_target() {
        let fool = Fool;
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
            fool.night_action(&ctx, &mut state),
            vec![NightAction::Investigate {
                target: PlayerId(2)
            }]
        );
    }
}
