//! Arsonist has one night action with two shapes, chosen the same way
//! Sandman's toggle is (Werewolf.cs:1013-1019, 3186-3188): pick a target to
//! douse in kerosene (`x.Doused`, no per-game or per-night limit — doused
//! players just accumulate), or flip to "Spark" (`player.Choice = -2`) and
//! detonate everyone doused so far. `NightContext::toggle_choice` carries
//! the spark decision; `chosen_target` carries a douse. Arsonist also
//! cannot be frozen/blocked by anything (Werewolf.cs:3187: "Fire beats
//! ice!"), which isn't a decision this file makes either way — it's the
//! *absence* of a gate a future orchestrator would otherwise apply.
//!
//! The full doused set *is* tracked, and the mass-burn resolution itself
//! (Werewolf.cs:3190-3202, `KillMthd.Burn`) **is** modeled — just not in
//! this file, or in `RoleState`. `RoleState` only has a single
//! `remembered_player` slot, not the `Vec<PlayerId>` a whole accumulated
//! set needs, so `game::run_game` keeps its own `doused: Vec<PlayerId>`
//! across rounds instead (the same "cross-round state lives in `run_game`,
//! not a role file" pattern as `silver_spread_tonight`/
//! `wolf_cub_bonus_pending`), and the Arsonist region of `night::resolve_night_sequence`
//! turns a `Detonate` action plus that set into `KillMethod::Burn` deaths.

use crate::roles::definition::*;
use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::Arsonist,
    behavior: &Arsonist,
    night: NightQuestion::DouseOrSpark,
    day: DayQuestion::None,
    info_key: "RoleInfoArsonist",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines {
        visited_serial_killer: Some("ArsonistVisitKiller"),
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::NonVillage,
        strength: |_| 8,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Arsonist;

impl RoleBehavior for Arsonist {
    fn night_action(&self, ctx: &NightContext, state: &mut RoleState) -> Vec<NightAction> {
        if ctx.toggle_choice {
            return vec![NightAction::Detonate];
        }
        match ctx.chosen_target {
            Some(target) if target != ctx.self_id && ctx.alive.contains(&target) => {
                state.remembered_player = Some(target);
                vec![NightAction::Douse { target }]
            }
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::PlayerId;

    fn ctx(target: Option<PlayerId>, toggle_choice: bool) -> NightContext<'static> {
        NightContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: target,
            heal_target: None,
            poison_target: None,
            love_targets: None,
            wolf_target: None,
            toggle_choice,
        }
    }

    #[test]
    fn arsonist_can_douse_a_target_any_night() {
        let arsonist = Arsonist;
        let mut state = RoleState::default();
        assert_eq!(
            arsonist.night_action(&ctx(Some(PlayerId(2)), false), &mut state),
            vec![NightAction::Douse { target: PlayerId(2) }]
        );
        assert_eq!(state.remembered_player, Some(PlayerId(2)));
    }

    #[test]
    fn arsonist_can_detonate_instead() {
        let arsonist = Arsonist;
        let mut state = RoleState::default();
        assert_eq!(
            arsonist.night_action(&ctx(None, true), &mut state),
            vec![NightAction::Detonate]
        );
    }
}
