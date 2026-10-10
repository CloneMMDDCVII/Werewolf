//! Augur has no choice to make at all (Werewolf.cs:4044-4060): every night
//! they're automatically told one role that is *not* in the game (picked
//! at random from the game's role pool, among roles not yet shown and not
//! held by any surviving player), or told nothing new once every such role
//! has been shown. There's no target and no toggle, so this role is
//! passive from `RoleBehavior`'s perspective; the reading and the
//! `SawRoles` memory live in `reveals::resolve_night_readings`.

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
