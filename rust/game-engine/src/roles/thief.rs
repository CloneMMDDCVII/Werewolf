//! Thief steals a player's role on the first night (Werewolf.cs:4136-4163).
//! In `ThiefFull` mode (`GameRules::thief_full`) the Thief tries every night,
//! with a chance of failing (Werewolf.cs:4164-4189).
//!
//! Resolved in:
//! - [`Night::thief`](crate::night::Night::thief) and [`Night::steal`](crate::night::Night::steal): the steal, in both modes.

use crate::roles::definition::*;
use crate::roles::{NightAction, NightContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::Thief,
    behavior: &Thief,
    night: NightQuestion::Pick(Prompt::ThiefSteal),
    day: DayQuestion::None,
    info_key: "RoleInfoThief",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 0,
    frozen_loses_action: false,
    deaths: DeathLines {
        fell_in_grave: "ThiefFellPublic",
        visited_serial_killer: Some("ThiefStoleKiller"),
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::NonVillage,
        // Werewolf.cs:296-298: "Testing 0 instead of 4 since thief
        // doesn't really seem to 'harm' the village team a lot" - the
        // source's own comment, kept verbatim since it explains a
        // deliberately-tuned value that looks like it could be a typo.
        strength: |_| 0,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Thief;

impl RoleBehavior for Thief {
    fn night_action(&self, ctx: &NightContext, state: &mut RoleState) -> Vec<NightAction> {
        if state.primary_used {
            return vec![];
        }
        match ctx.chosen_target {
            Some(target) if target != ctx.self_id && ctx.alive.contains(&target) => {
                state.primary_used = true;
                vec![NightAction::StealRole {
                    thief: ctx.self_id,
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
            alive: &[PlayerId(1), PlayerId(2), PlayerId(3)],
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
    fn thief_steals_a_role_once() {
        let thief = Thief;
        let mut state = RoleState::default();
        assert_eq!(
            thief.night_action(&ctx(Some(PlayerId(2))), &mut state),
            vec![NightAction::StealRole {
                thief: PlayerId(1),
                target: PlayerId(2)
            }]
        );
        assert!(state.primary_used);
        assert_eq!(thief.night_action(&ctx(Some(PlayerId(3))), &mut state), vec![]);
    }

    #[test]
    fn thief_cannot_steal_from_itself() {
        let thief = Thief;
        let mut state = RoleState::default();
        assert_eq!(thief.night_action(&ctx(Some(PlayerId(1))), &mut state), vec![]);
    }
}
