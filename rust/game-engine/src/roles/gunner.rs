//! Gunner has two bullets (Werewolf.cs:1938: `bullet: 2`), spent one at a
//! time to shoot a suspected player during the **day** phase
//! (Werewolf.cs:2871-2896) — this is what motivated adding the
//! `day_action` hook: forcing this through `night_action` would have been
//! wrong regardless of how convenient it looked.
//!
//! Reuses `RoleState::primary_used`/`secondary_used` for "first bullet
//! spent"/"second bullet spent", the same trick `witch::Witch` uses for
//! her two potions.
//!
//! The shot always kills its target outright (`KillMethod::Shoot`, in
//! `orchestrator::apply_day_results`) — including a Wise Elder, who does
//! *not* survive it (checked directly: `KillPlayer(check, ...)` runs
//! unconditionally after the Wise Elder switch case, Werewolf.cs:2895).
//! The real special case is what happens to the *Gunner* afterward: they
//! get demoted to Villager (Werewolf.cs:2887-2889), which `game::run_game`
//! applies using the `shooter` field on `DayAction::Shoot` — this file
//! only covers "is there a bullet left, and is this a valid target,"
//! same as before.

use crate::roles::definition::*;
use crate::roles::{DayAction, DayContext, RoleBehavior, RoleState};

pub const DEF: RoleDef = RoleDef {
    role: Role::Gunner,
    behavior: &Gunner,
    night: NightQuestion::None,
    day: DayQuestion::Shoot,
    info_key: "RoleInfoGunner",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines {
        eaten: "GunnerEaten",
        serial_killed: "GunnerKilled",
        ..DeathLines::DEFAULT
    },
    balance: Balance {
        group: BalanceGroup::RevealedVillage,
        strength: |_| 6,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Gunner;

/// Bullets a Gunner has left (Werewolf.cs:1938: `bullet: 2`). Derived from
/// the two "spent" flags, so a Thief who steals a Gunner and inherits
/// their `RoleState` (Werewolf.cs:2479-2486 passes `bullet: targetBullet`)
/// keeps the same count.
pub fn bullets_left(state: &RoleState) -> u8 {
    2 - state.primary_used as u8 - state.secondary_used as u8
}

impl RoleBehavior for Gunner {
    fn day_action(&self, ctx: &DayContext, state: &mut RoleState) -> Vec<DayAction> {
        if state.primary_used && state.secondary_used {
            return vec![];
        }
        match ctx.chosen_target {
            Some(target) if target != ctx.self_id && ctx.alive.contains(&target) => {
                if !state.primary_used {
                    state.primary_used = true;
                } else {
                    state.secondary_used = true;
                }
                vec![DayAction::Shoot {
                    shooter: ctx.self_id,
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
            alive: &[PlayerId(1), PlayerId(2), PlayerId(3)][..],
            self_id: PlayerId(1),
            chosen_target: target,
            toggle_choice: false,
        }
    }

    #[test]
    fn gunner_can_fire_both_bullets_across_two_days() {
        let gunner = Gunner;
        let mut state = RoleState::default();

        let first = gunner.day_action(&ctx(Some(PlayerId(2))), &mut state);
        assert_eq!(
            first,
            vec![DayAction::Shoot {
                shooter: PlayerId(1),
                target: PlayerId(2)
            }]
        );
        assert!(state.primary_used && !state.secondary_used);

        let second = gunner.day_action(&ctx(Some(PlayerId(3))), &mut state);
        assert_eq!(
            second,
            vec![DayAction::Shoot {
                shooter: PlayerId(1),
                target: PlayerId(3)
            }]
        );
        assert!(state.primary_used && state.secondary_used);
        assert_eq!(bullets_left(&state), 0);
    }

    #[test]
    fn gunner_cannot_fire_a_third_shot() {
        let gunner = Gunner;
        let mut state = RoleState {
            primary_used: true,
            secondary_used: true,
            ..Default::default()
        };
        assert_eq!(gunner.day_action(&ctx(Some(PlayerId(2))), &mut state), vec![]);
    }

    #[test]
    fn gunner_cannot_shoot_itself() {
        let gunner = Gunner;
        let mut state = RoleState::default();
        assert_eq!(gunner.day_action(&ctx(Some(PlayerId(1))), &mut state), vec![]);
    }
}
