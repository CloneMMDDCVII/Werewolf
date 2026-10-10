//! Wolf Man is Village team and has no action of his own — his entire
//! gimmick is that other roles get the *wrong* answer about him
//! (Werewolf.cs:3946: "poor wolf man, is just a villager!"). The Seer
//! and Fool both see him reported as `Wolf` (Werewolf.cs:3946-3948,
//! 4008), a false positive, not a real detection ability of his own.
//! That's a display-layer effect on *other* roles' `Investigate`/
//! `CheckTeam` results, which this proof-of-concept doesn't model (same
//! category of gap as `lycan::Lycan`'s reverse case: a real Wolf who
//! reads as Villager).

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
