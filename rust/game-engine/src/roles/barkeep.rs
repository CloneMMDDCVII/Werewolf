//! Barkeep, like `chef::Chef`, doesn't exist in this project's
//! `Werewolf.cs` — same beta-fork-only status, same honest "no source, no
//! invented mechanic" treatment.

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::Barkeep,
    behavior: &Barkeep,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoBarkeep",
    seer_sees: SeerSees::Itself,
    sorcerer_sees: SorcererSees::Nothing,
    cult_conversion: 100,
    frozen_loses_action: false,
    deaths: DeathLines::DEFAULT,
    balance: Balance {
        group: BalanceGroup::Village,
        strength: |_| 0,
        pool: Pool::Never,
    },
    dealt: true,
};

pub struct Barkeep;

impl RoleBehavior for Barkeep {}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::Team;

    #[test]
    fn barkeep_is_village_team() {
        assert_eq!(Role::Barkeep.team(), Team::Village);
    }
}
