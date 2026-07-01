//! New role, not in the legacy game (see `shared::Role::Witch` doc
//! comment). Two independent one-shot potions: heal (cancel tonight's
//! wolf kill) and poison (kill a target outright), each usable once per
//! game. This is the file that justifies `night_action` returning a `Vec`
//! instead of a single `Option<NightAction>` — Villager/Wolf/Seer only
//! ever need at most one action, but the Witch can use both potions on
//! the same night.
//!
//! **Gated off** (`is_available() == false`): the heal potion only makes
//! sense as "save the wolves' victim," which means Witch can't be asked
//! her question until the wolf vote has actually resolved — a genuine
//! information dependency, declared via `requires()`, not a difficulty
//! rating. There's no orchestrator yet to resolve night actions in
//! dependency order, so shipping her live would mean asking the heal
//! question with a `wolf_target` that's always `None` — the logic below
//! is real and tested, it's just not wired to a table yet.

use crate::roles::{NightAction, NightContext, NightFact, RoleBehavior, RoleState};
use shared::{Role, Team};

pub struct Witch;

impl RoleBehavior for Witch {
    fn team(&self) -> Team {
        Role::Witch.team()
    }

    fn night_action(&self, ctx: &NightContext, state: &mut RoleState) -> Vec<NightAction> {
        let mut actions = vec![];

        // Heal only ever targets the wolves' victim — that's the whole
        // point of the potion — so it's only valid when it matches the
        // resolved `wolf_target`, not an arbitrary free choice.
        if !state.primary_used {
            if let (Some(target), Some(wolf_target)) = (ctx.heal_target, ctx.wolf_target) {
                if target == wolf_target && ctx.alive.contains(&target) {
                    state.primary_used = true;
                    actions.push(NightAction::Heal { target });
                }
            }
        }

        // Poison has no such dependency: any living player, any night.
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

    fn requires(&self) -> &'static [NightFact] {
        &[NightFact::WolfTarget]
    }

    fn is_available(&self) -> bool {
        false
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
            wolf_target: Some(PlayerId(2)),
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
            wolf_target: Some(PlayerId(2)),
        };
        let mut state = RoleState {
            primary_used: true,
            secondary_used: false,
        };
        assert_eq!(witch.night_action(&ctx, &mut state), vec![]);
    }

    /// The load-bearing test: heal doesn't fire just because a target was
    /// chosen — it needs the wolf's target to actually be known and to
    /// match. This is what makes `requires()` more than decorative.
    #[test]
    fn witch_cannot_heal_without_a_resolved_wolf_target() {
        let witch = Witch;
        let ctx = NightContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: None,
            heal_target: Some(PlayerId(2)),
            poison_target: None,
            love_targets: None,
            wolf_target: None,
        };
        let mut state = RoleState::default();
        assert_eq!(witch.night_action(&ctx, &mut state), vec![]);
        assert!(!state.primary_used, "potion should not be spent on a no-op");
    }

    #[test]
    fn witch_cannot_heal_someone_other_than_the_wolves_target() {
        let witch = Witch;
        let ctx = NightContext {
            alive: &[PlayerId(1), PlayerId(2), PlayerId(3)],
            self_id: PlayerId(1),
            chosen_target: None,
            heal_target: Some(PlayerId(2)),
            poison_target: None,
            love_targets: None,
            wolf_target: Some(PlayerId(3)),
        };
        let mut state = RoleState::default();
        assert_eq!(witch.night_action(&ctx, &mut state), vec![]);
    }

    #[test]
    fn witch_declares_a_dependency_on_the_wolf_target() {
        assert_eq!(Witch.requires(), &[NightFact::WolfTarget]);
    }

    #[test]
    fn witch_is_not_yet_available_to_players() {
        assert!(!Witch.is_available());
    }
}
