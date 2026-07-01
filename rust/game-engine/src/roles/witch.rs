//! New role, not in the legacy game (see `shared::Role::Witch` doc
//! comment). Two independent one-shot potions: heal (cancel tonight's
//! wolf kill on a target) and poison (kill a target outright), each usable
//! once per game. This is the file that justifies `night_action` returning
//! a `Vec` instead of a single `Option<NightAction>` — Villager/Wolf/Seer
//! only ever need at most one action, but the Witch can use both potions
//! on the same night.

use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Witch;

impl RoleBehavior for Witch {
    fn team(&self) -> Team {
        Role::Witch.team()
    }

    fn night_action(&self, ctx: &NightContext, state: &mut RoleState) -> Vec<NightAction> {
        let mut actions = vec![];

        if !state.primary_used {
            if let Some(target) = ctx.heal_target {
                if ctx.alive.contains(&target) {
                    state.primary_used = true;
                    actions.push(NightAction::Heal { target });
                }
            }
        }

        if !state.secondary_used {
            if let Some(target) = ctx.poison_target {
                if ctx.alive.contains(&target) {
                    state.secondary_used = true;
                    actions.push(NightAction::Poison { target });
                }
            }
        }

        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::PlayerId;

    #[test]
    fn witch_can_use_both_potions_the_same_night() {
        let witch = Witch;
        let ctx = NightContext {
            alive: &[PlayerId(1), PlayerId(2), PlayerId(3)],
            self_id: PlayerId(1),
            chosen_target: None,
            heal_target: Some(PlayerId(2)),
            poison_target: Some(PlayerId(3)),
            love_targets: None,
        };
        let mut state = RoleState::default();
        let actions = witch.night_action(&ctx, &mut state);
        assert_eq!(actions.len(), 2);
        assert!(state.primary_used);
        assert!(state.secondary_used);
    }

    #[test]
    fn witch_cannot_reuse_a_potion_already_spent() {
        let witch = Witch;
        let ctx = NightContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: None,
            heal_target: Some(PlayerId(2)),
            poison_target: None,
            love_targets: None,
        };
        let mut state = RoleState {
            primary_used: true,
            secondary_used: false,
        };
        assert_eq!(witch.night_action(&ctx, &mut state), vec![]);
    }
}
