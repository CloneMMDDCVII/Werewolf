//! The Seer checks one player's role each night. Structurally identical to
//! `wolf::Wolf` (validate a chosen target, emit one action) — worth
//! comparing the two files side by side as a second example once you've
//! read `villager.rs`. What the Seer is told, including the legacy
//! disguises (Lycan reads as Villager, Wolf Man as Wolf), is resolved in
//! `reveals::resolve_night_readings`.

use crate::roles::definition::*;
use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::Seer,
    behavior: &Seer,
    night: NightQuestion::Pick(Prompt::SeerCheck),
    day: DayQuestion::None,
    info_key: "RoleInfoSeer",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Itself,
    cult_conversion: 40,
    frozen_loses_action: true,
    deaths: DeathLines {
        eaten: "SeerEaten",
        serial_killed: "SeerKilled",
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
    let count = |r: Role| all_roles.iter().filter(|&&x| x == r).count() as i32;
    7 - count(Role::Lycan) - (count(Role::WolfMan) * 2)
}

pub struct Seer;

impl RoleBehavior for Seer {
    fn night_action(&self, ctx: &NightContext, _state: &mut RoleState) -> Vec<NightAction> {
        match ctx.chosen_target {
            Some(target) if target != ctx.self_id && ctx.alive.contains(&target) => {
                vec![NightAction::CheckTeam { target }]
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
    fn seer_checks_a_valid_target() {
        let seer = Seer;
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
            seer.night_action(&ctx, &mut state),
            vec![NightAction::CheckTeam { target: PlayerId(2) }]
        );
    }
}
