//! Wolf Man is on the village team and has no action. The Seer and the Fool
//! see him as a Wolf (Werewolf.cs:3946-3948, 4008).
//!
//! Resolved in:
//! - `DEF.seer_sees`, read by [`reveals::resolve_night_readings`](crate::reveals::resolve_night_readings).

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::WolfMan,
    behavior: &WolfMan,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoWolfMan",
    seer_sees: SeerSees::As(Role::Wolf),
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines::DEFAULT,
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 1,
        pool: Pool::Always,
    },
    dealt: true,
};

pub struct WolfMan;

impl RoleBehavior for WolfMan {}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::Team;

    #[test]
    fn wolf_man_is_village_team_despite_reading_as_wolf_to_seers() {
        assert_eq!(Role::WolfMan.team(), Team::Village);
    }
}
