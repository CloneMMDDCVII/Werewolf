//! Cultist Hunter checks one player overnight, hunting specifically for
//! Cultists (Werewolf.cs:3559 onward). The hunt itself, killing a Cultist
//! or reporting a miss, is the Cultist Hunter region of `night::resolve_night_sequence`.

use crate::roles::definition::*;
use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::CultistHunter,
    behavior: &CultistHunter,
    night: NightQuestion::Pick(Prompt::CultistHunterInvestigate),
    day: DayQuestion::None,
    info_key: "RoleInfoCultistHunter",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: true,
    deaths: DeathLines {
        visited_serial_killer: Some("SerialKillerKilledCH"),
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::Village,
        strength,
        pool: Pool::OverTenPlayers,
    },
    dealt: true,
};

/// `GameBalancing.cs::GetStrength`.
fn strength(all_roles: &[Role]) -> i32 {
    let count = |r: Role| all_roles.iter().filter(|&&x| x == r).count() as i32;
    if count(Role::Cultist) == 0 {
        1
    } else {
        7
    }
}

pub struct CultistHunter;

impl RoleBehavior for CultistHunter {
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
    fn cultist_hunter_investigates_a_valid_target() {
        let ch = CultistHunter;
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
            ch.night_action(&ctx, &mut state),
            vec![NightAction::Investigate { target: PlayerId(2) }]
        );
    }
}
