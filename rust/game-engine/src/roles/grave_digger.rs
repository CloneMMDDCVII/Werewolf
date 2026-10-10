//! Grave Digger has no action. Once they have dug a grave, anyone visiting
//! them at night may fall in and die (Werewolf.cs:2377-2411). A Serial
//! Killer never falls in (Werewolf.cs:2382-2387) but stumbles, which costs
//! them a later night.
//!
//! Resolved in:
//! - [`Night::visit`](crate::night::Night::visit): every fall;
//! - [`Night::spot_grave_digger`](crate::night::Night::spot_grave_digger): the wolves spotting the digger instead;
//! - the night step of [`game::run_game_with_rules`](crate::game::run_game_with_rules): counting dug graves
//!   (Werewolf.cs:5294-5299).

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::GraveDigger,
    behavior: &GraveDigger,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoGraveDigger",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 30,
    frozen_loses_action: true,
    deaths: DeathLines::DEFAULT,
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 5,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct GraveDigger;

impl RoleBehavior for GraveDigger {}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::Team;

    #[test]
    fn grave_digger_is_village_team() {
        assert_eq!(Role::GraveDigger.team(), Team::Village);
    }
}
