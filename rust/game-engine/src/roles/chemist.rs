//! Chemist brews one night, then visits a target the next with two potions,
//! one lethal (Werewolf.cs:3792-3821): either the target or the Chemist dies.
//! There is no limit on how often.
//!
//! Resolved in:
//! - [`orchestrator::night_ask::resolve_night_with_freeze`](crate::orchestrator::night_ask::resolve_night_with_freeze): brewing instead of asking;
//! - [`Night::chemist`](crate::night::Night::chemist) and [`Night::chemist_killed_wise_elder`](crate::night::Night::chemist_killed_wise_elder): the gamble.

use crate::roles::definition::*;
use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::Chemist,
    behavior: &Chemist,
    night: NightQuestion::BrewThenPick(Prompt::ChemistBrew),
    day: DayQuestion::None,
    info_key: "RoleInfoChemist",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: true,
    deaths: DeathLines {
        visited_serial_killer: Some("ChemistSKPublic"),
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 0,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Chemist;

impl RoleBehavior for Chemist {
    fn night_action(&self, ctx: &NightContext, _state: &mut RoleState) -> Vec<NightAction> {
        match ctx.chosen_target {
            Some(target) if target != ctx.self_id && ctx.alive.contains(&target) => {
                vec![NightAction::Chemistry {
                    chemist: ctx.self_id,
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

    fn ctx(target: Option<PlayerId>) -> NightContext<'static> {
        NightContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: target,
            heal_target: None,
            poison_target: None,
            love_targets: None,
            wolf_target: None,
            toggle_choice: false,
        }
    }

    #[test]
    fn chemist_can_gamble_on_a_target_every_night() {
        let chemist = Chemist;
        let mut state = RoleState::default();
        assert_eq!(
            chemist.night_action(&ctx(Some(PlayerId(2))), &mut state),
            vec![NightAction::Chemistry {
                chemist: PlayerId(1),
                target: PlayerId(2)
            }]
        );
        assert_eq!(
            chemist.night_action(&ctx(Some(PlayerId(2))), &mut state),
            vec![NightAction::Chemistry {
                chemist: PlayerId(1),
                target: PlayerId(2)
            }],
            "no per-game usage limit, unlike Gunner or Witch"
        );
    }

    #[test]
    fn chemist_cannot_target_itself() {
        let chemist = Chemist;
        let mut state = RoleState::default();
        assert_eq!(chemist.night_action(&ctx(Some(PlayerId(1))), &mut state), vec![]);
    }
}
