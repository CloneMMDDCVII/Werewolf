//! Fool checks one player overnight like the Seer (Werewolf.cs:5174-5179), but
//! is shown a random role, with wolf variants shown as Wolf
//! (Werewolf.cs:3985-4000).
//!
//! Resolved in:
//! - [`reveals::resolve_night_readings`](crate::reveals::resolve_night_readings): the fake reading.

use crate::roles::definition::*;
use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::Fool,
    behavior: &Fool,
    night: NightQuestion::Pick(Prompt::FoolInvestigate),
    day: DayQuestion::None,
    info_key: "RoleInfoSeer",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: true,
    deaths: DeathLines {
        eaten: "FoolEaten",
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 3,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Fool;

impl RoleBehavior for Fool {
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
            toggle_choice: false,
        };
        let mut state = RoleState::default();
        assert_eq!(
            fool.night_action(&ctx, &mut state),
            vec![NightAction::Investigate { target: PlayerId(2) }]
        );
    }
}
