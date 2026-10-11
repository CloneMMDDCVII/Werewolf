//! Spumpkin picks a target by day and explodes with a 40% chance, killing both
//! (Werewolf.cs:2901-2930). A failed attempt can be tried again the next day
//! (Werewolf.cs:5116-5130).
//!
//! Resolved in:
//! - [`orchestrator::deaths::resolve_spumpkin_detonation`](crate::orchestrator::deaths::resolve_spumpkin_detonation): the roll and the deaths.

use crate::roles::definition::*;
use crate::roles::{DayAction, DayContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::Spumpkin,
    behavior: &Spumpkin,
    night: NightQuestion::None,
    day: DayQuestion::Pick(Prompt::SpumpkinDetonate),
    info_key: "RoleInfoSpumpkin",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 0,
    frozen_loses_action: false,
    deaths: DeathLines::DEFAULT,
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 2,
        pool: Pool::Never,
    },
    dealt: true,
};

pub struct Spumpkin;

impl RoleBehavior for Spumpkin {
    fn day_action(&self, ctx: &DayContext, _state: &mut RoleState) -> Vec<DayAction> {
        match ctx.chosen_target {
            Some(target) if target != ctx.self_id && ctx.alive.contains(&target) => {
                vec![DayAction::Detonate {
                    spumpkin: ctx.self_id,
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

    fn ctx(target: Option<PlayerId>) -> DayContext<'static> {
        DayContext {
            alive: &[PlayerId(1), PlayerId(2)],
            self_id: PlayerId(1),
            chosen_target: target,
            toggle_choice: false,
        }
    }

    #[test]
    fn spumpkin_can_try_to_detonate_every_day_until_it_lands() {
        let spumpkin = Spumpkin;
        let mut state = RoleState::default();
        assert_eq!(
            spumpkin.day_action(&ctx(Some(PlayerId(2))), &mut state),
            vec![DayAction::Detonate {
                spumpkin: PlayerId(1),
                target: PlayerId(2)
            }]
        );
        assert_eq!(
            spumpkin.day_action(&ctx(Some(PlayerId(2))), &mut state),
            vec![DayAction::Detonate {
                spumpkin: PlayerId(1),
                target: PlayerId(2)
            }],
            "a failed (60%) attempt isn't a per-game use, unlike Gunner's bullets"
        );
    }

    #[test]
    fn spumpkin_cannot_detonate_itself() {
        let spumpkin = Spumpkin;
        let mut state = RoleState::default();
        assert_eq!(spumpkin.day_action(&ctx(Some(PlayerId(1))), &mut state), vec![]);
    }
}
