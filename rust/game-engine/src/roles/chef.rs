//! Chef doesn't exist in this project's `Werewolf.cs` at all — confirmed by
//! an exhaustive case-insensitive search of the legacy source, not just an
//! oversight in earlier porting batches. `shared::Role::team`'s doc already
//! flags Chef/Barkeep as verified against a different fork
//! (GreyWolfDev/Werewolf@beta) instead. With no local source to cite line
//! numbers against, this file only asserts the team already established
//! there — no action to model, honestly, rather than invented.

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
