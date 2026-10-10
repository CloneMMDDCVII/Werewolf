//! Oracle checks one player overnight — a "negative" Seer, revealing a
//! decoy role that isn't the target's (Werewolf.cs:4021-4034) rather than
//! the target's actual team. Same shape as every other investigate-style
//! role: validate a target, emit `Investigate`; what comes back is
//! resolution/display logic this file doesn't attempt.

use crate::roles::definition::*;
use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::Oracle,
    behavior: &Oracle,
    night: NightQuestion::Pick(Prompt::OracleInvestigate),
    day: DayQuestion::None,
    info_key: "RoleInfoOracle",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 50,
    frozen_loses_action: true,
    deaths: DeathLines::DEFAULT,
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 4,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Oracle;

impl RoleBehavior for Oracle {
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
    fn oracle_investigates_a_valid_target() {
        let oracle = Oracle;
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
            oracle.night_action(&ctx, &mut state),
            vec![NightAction::Investigate { target: PlayerId(2) }]
        );
    }
}
