//! New role, not in the legacy game (see `shared::Role::Witch` doc
//! comment). Two independent one-shot potions: heal (cancel tonight's
//! wolf kill) and poison (kill a target outright), each usable once per
//! game. This is the file that justifies `night_action` returning a `Vec`
//! instead of a single `Option<NightAction>` — Villager/Wolf/Seer only
//! ever need at most one action, but the Witch can use both potions on
//! the same night.
//!
//! The heal only makes sense as "save the wolves' victim", so the Witch is
//! asked after the wolves' vote (`NightQuestion::AfterWolves`). She is not
//! dealt (`dealt: false`): she has no legacy texts or balancing weight.
//! Her potions are resolved in the Witch region of `night`.

use crate::roles::definition::*;
use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::Witch,
    behavior: &Witch,
    night: NightQuestion::AfterWolves,
    day: DayQuestion::None,
    info_key: "RoleInfoWitch",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines::DEFAULT,
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 0,
        pool: Pool::Never,
    },
    dealt: false,
};

pub struct Witch;

impl RoleBehavior for Witch {
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
            toggle_choice: false,
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
            toggle_choice: false,
        };
        let mut state = RoleState {
            primary_used: true,
            secondary_used: false,
            ..Default::default()
        };
        assert_eq!(witch.night_action(&ctx, &mut state), vec![]);
    }

    /// The load-bearing test: heal doesn't fire just because a target was
    /// chosen — it needs the wolf's target to actually be known and to
    /// match, which is why she is asked after the wolves.
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
            toggle_choice: false,
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
            toggle_choice: false,
        };
        let mut state = RoleState::default();
        assert_eq!(witch.night_action(&ctx, &mut state), vec![]);
    }
}
