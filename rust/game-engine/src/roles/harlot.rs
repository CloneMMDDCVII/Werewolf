//! Harlot visits one player overnight, or stays home by picking herself
//! (Werewolf.cs:3172-3175). She dies visiting a wolf or the wolves' victim
//! (Werewolf.cs:2358-2360), and she is not home if the wolves come for her.
//!
//! Resolved in:
//! - [`Night::harlot`](crate::night::Night::harlot): the visit;
//! - [`Night::wolves`](crate::night::Night::wolves) and [`Night::visit`](crate::night::Night::visit): not being home.

use crate::roles::definition::*;
use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::Harlot,
    behavior: &Harlot,
    night: NightQuestion::Pick(Prompt::HarlotVisit),
    day: DayQuestion::None,
    info_key: "RoleInfoHarlot",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 70,
    frozen_loses_action: true,
    deaths: DeathLines {
        eaten: "HarlotEaten",
        fell_in_grave: "HarlotFellPublic",
        visited_serial_killer: Some("HarlotFuckKillerPublic"),
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 6,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Harlot;

impl RoleBehavior for Harlot {
    fn night_action(&self, ctx: &NightContext, _state: &mut RoleState) -> Vec<NightAction> {
        match ctx.chosen_target {
            Some(target) if ctx.alive.contains(&target) => {
                vec![NightAction::Visit {
                    visitor: ctx.self_id,
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

    #[test]
    fn harlot_may_visit_herself_to_stay_home() {
        let harlot = Harlot;
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
        assert_eq!(
            harlot.night_action(&ctx, &mut state),
            vec![NightAction::Visit {
                visitor: PlayerId(1),
                target: PlayerId(1)
            }]
        );
    }

    #[test]
    fn harlot_cannot_visit_a_dead_player() {
        let harlot = Harlot;
        let ctx = NightContext {
            alive: &[PlayerId(1)],
            self_id: PlayerId(1),
            chosen_target: Some(PlayerId(2)),
            heal_target: None,
            poison_target: None,
            love_targets: None,
            wolf_target: None,
            toggle_choice: false,
        };
        let mut state = RoleState::default();
        assert_eq!(harlot.night_action(&ctx, &mut state), vec![]);
    }
}
