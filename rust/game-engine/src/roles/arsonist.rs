//! Arsonist either douses one more house in kerosene or sparks, burning
//! everyone doused so far (Werewolf.cs:1013-1019, 3186-3202). A freeze does not
//! stop the Arsonist (Werewolf.cs:3187, "Fire beats ice!").
//!
//! Resolved in:
//! - [`orchestrator::night_ask::resolve_night_with_freeze`](crate::orchestrator::night_ask::resolve_night_with_freeze): the douse-or-spark question;
//! - [`Night::arsonist`](crate::night::Night::arsonist): the burn, and the doused set kept in
//!   [`NightMemory::doused`](crate::night::NightMemory::doused).

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
