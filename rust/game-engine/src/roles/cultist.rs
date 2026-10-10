//! Cultists vote overnight on one player to convert (Werewolf.cs:3591-3757).
//! Whether the conversion lands depends on the target's role
//! (`RoleDef::cult_conversion` on each role).
//!
//! Resolved in:
//! - [`Night::cult`](crate::night::Night::cult), [`Night::vote_cult`](crate::night::Night::vote_cult) and [`Night::convert`](crate::night::Night::convert): the vote, the target's
//!   resistance and the newest Cultist's risky visit.

use crate::roles::definition::*;
use crate::roles::{NightAction, NightContext, PlayerId, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::Cultist,
    behavior: &Cultist,
    night: NightQuestion::Pick(Prompt::CultistConvert),
    day: DayQuestion::None,
    info_key: "RoleInfoCultist",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: true,
    deaths: DeathLines {
        serial_killed: "CultistKilled",
        fell_in_grave: "CultistFellPublic",
        visited_serial_killer: Some("CultConvertKillerPublic"),
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::NonVillage,
        strength,
        pool: Pool::OverTenPlayers,
    },
    dealt: true,
};

/// `GameBalancing.cs::GetStrength`.
fn strength(all_roles: &[Role]) -> i32 {
    10 + all_roles.iter().filter(|&&x| !NON_CONVERTIBLE.contains(&x)).count() as i32
}

/// `GameBalancing.cs::GetStrength`'s roles the cult can't win over.
const NON_CONVERTIBLE: [Role; 13] = [
    Role::Seer,
    Role::GuardianAngel,
    Role::Detective,
    Role::Cursed,
    Role::Harlot,
    Role::Hunter,
    Role::Doppelganger,
    Role::Wolf,
    Role::AlphaWolf,
    Role::WolfCub,
    Role::SerialKiller,
    Role::Lycan,
    Role::Thief,
];

pub struct Cultist;

impl RoleBehavior for Cultist {
    fn night_action(&self, ctx: &NightContext, _state: &mut RoleState) -> Vec<NightAction> {
        match ctx.chosen_target {
            Some(target) if is_valid_target(ctx, target) => {
                vec![NightAction::ConvertVote { target }]
            }
            _ => vec![],
        }
    }
}

fn is_valid_target(ctx: &NightContext, target: PlayerId) -> bool {
    target != ctx.self_id && ctx.alive.contains(&target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cultist_proposes_a_valid_conversion_target() {
        let cultist = Cultist;
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
            cultist.night_action(&ctx, &mut state),
            vec![NightAction::ConvertVote { target: PlayerId(2) }]
        );
    }

    #[test]
    fn cultist_cannot_convert_itself() {
        let cultist = Cultist;
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
        assert_eq!(cultist.night_action(&ctx, &mut state), vec![]);
    }
}
