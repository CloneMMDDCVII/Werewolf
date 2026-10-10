//! Chef is not in this project's `Werewolf.cs`; it comes from the
//! GreyWolfDev/Werewolf beta fork. It has a team and no mechanic, rather than an
//! invented one. The balancer never deals it (`Pool::Never`).

use crate::roles::definition::*;
use crate::roles::RoleBehavior;

pub const DEF: RoleDef = RoleDef {
    role: Role::Chef,
    behavior: &Chef,
    night: NightQuestion::None,
    day: DayQuestion::None,
    info_key: "RoleInfoChef",
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

pub struct Chef;

impl RoleBehavior for Chef {}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::Team;

    #[test]
    fn chef_is_village_team() {
        assert_eq!(Role::Chef.team(), Team::Village);
    }
}
