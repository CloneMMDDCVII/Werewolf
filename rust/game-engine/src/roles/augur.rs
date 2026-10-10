//! Augur makes no choice: each night they are told one role that is not in
//! the game, or nothing once every such role has been shown
//! (Werewolf.cs:4044-4060).
//!
//! Resolved in:
//! - [`reveals::resolve_night_readings`](crate::reveals::resolve_night_readings): the reading and its memory.

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::Augur,
    behavior: &Augur,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoAugur",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 40,
    frozen_loses_action: true,
    deaths: DeathLines::DEFAULT,
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 5,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct Augur;

impl RoleBehavior for Augur {}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::Team;

    #[test]
    fn augur_is_village_team() {
        assert_eq!(Role::Augur.team(), Team::Village);
    }
}
