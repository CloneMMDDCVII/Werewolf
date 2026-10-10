//! Guardian Angel protects one player overnight (Werewolf.cs:3091,
//! `AskGuard`). Self-protection is allowed.
//!
//! What the protection does (saving the wolves' target, the GA's risk
//! guarding a wolf, blocking the Snow Wolf's freeze) is resolved in the
//! Guardian Angel region of `night` and in `freeze`.

use crate::roles::definition::*;
use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::GuardianAngel,
    behavior: &GuardianAngel,
    night: NightQuestion::Pick(Prompt::GuardianAngelProtect),
    day: DayQuestion::None,
    info_key: "RoleInfoGuardianAngel",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 60,
    frozen_loses_action: true,
    deaths: DeathLines {
        eaten: "GuardianEaten",
        serial_killed: "GuardianAngelKilled",
        visited_serial_killer: Some("GAGuardedKiller"),
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::Village,
        strength,
        pool: Pool::Always,
    },
    dealt: true,
};

/// `GameBalancing.cs::GetStrength`.
fn strength(all_roles: &[Role]) -> i32 {
    7 + i32::from(all_roles.contains(&Role::Arsonist))
}

pub struct GuardianAngel;

impl RoleBehavior for GuardianAngel {
    fn night_action(&self, ctx: &NightContext, _state: &mut RoleState) -> Vec<NightAction> {
        match ctx.chosen_target {
            Some(target) if ctx.alive.contains(&target) => {
                vec![NightAction::Protect { target }]
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
    fn guardian_angel_may_protect_herself() {
        let ga = GuardianAngel;
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
            ga.night_action(&ctx, &mut state),
            vec![NightAction::Protect { target: PlayerId(1) }]
        );
    }

    #[test]
    fn guardian_angel_cannot_protect_a_dead_player() {
        let ga = GuardianAngel;
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
        assert_eq!(ga.night_action(&ctx, &mut state), vec![]);
    }
}
