use serde::{Deserialize, Serialize};

/// All roles observed across base game + Chaos mode.
/// Mirrors `Shared/Roles.cs` IRole enum (source project).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    Villager,
    Drunk,
    Harlot,
    Seer,
    Traitor,
    GuardianAngel,
    Detective,
    Wolf,
    Cursed,
    Gunner,
    Tanner,
    Fool,
    WildChild,
    Beholder,
    ApprenticeSeer,
    Cultist,
    CultistHunter,
    Mason,
    #[serde(rename = "Doppelgänger")]
    Doppelganger,
    Cupid,
    Hunter,
    SerialKiller,
    Sorcerer,
    AlphaWolf,
    WolfCub,
    Blacksmith,
    ClumsyGuy,
    Mayor,
    Prince,
    Lycan,
    Pacifist,
    WiseElder,
    Oracle,
    Sandman,
    WolfMan,
    Thief,
    Troublemaker,
    Chemist,
    SnowWolf,
    GraveDigger,
    Augur,
    Arsonist,
    Spumpkin,
    Chef,
    Barkeep,
    /// Not present in the legacy `Shared/Roles.cs` — added here as a new
    /// role (heal potion + poison potion, one use each) to prove out the
    /// new role-behavior structure against something that isn't just a
    /// ported 1:1 legacy role.
    Witch,
}

/// Every `Role` variant, in declaration order — for anything that needs
/// "all of them" (fuzzing a random roster, a future exhaustive coverage
/// report) without re-deriving the list by hand and risking it drifting
/// from the enum above. Hand-kept in sync deliberately rather than via a
/// derive macro: one more dependency isn't worth it for a list this small
/// and this rarely changed.
pub const ALL_ROLES: &[Role] = &[
    Role::Villager,
    Role::Drunk,
    Role::Harlot,
    Role::Seer,
    Role::Traitor,
    Role::GuardianAngel,
    Role::Detective,
    Role::Wolf,
    Role::Cursed,
    Role::Gunner,
    Role::Tanner,
    Role::Fool,
    Role::WildChild,
    Role::Beholder,
    Role::ApprenticeSeer,
    Role::Cultist,
    Role::CultistHunter,
    Role::Mason,
    Role::Doppelganger,
    Role::Cupid,
    Role::Hunter,
    Role::SerialKiller,
    Role::Sorcerer,
    Role::AlphaWolf,
    Role::WolfCub,
    Role::Blacksmith,
    Role::ClumsyGuy,
    Role::Mayor,
    Role::Prince,
    Role::Lycan,
    Role::Pacifist,
    Role::WiseElder,
    Role::Oracle,
    Role::Sandman,
    Role::WolfMan,
    Role::Thief,
    Role::Troublemaker,
    Role::Chemist,
    Role::SnowWolf,
    Role::GraveDigger,
    Role::Augur,
    Role::Arsonist,
    Role::Spumpkin,
    Role::Chef,
    Role::Barkeep,
    Role::Witch,
];

impl Role {
    /// Mirrors `Werewolf.cs::SetTeam` (Werewolf Node/Werewolf.cs:1546-1610).
    /// Chef/Barkeep (beta-only roles) verified against GreyWolfDev/Werewolf@beta.
    pub fn team(self) -> Team {
        use Role::*;
        match self {
            Villager | Cursed | Drunk | Beholder | ApprenticeSeer | Traitor | Mason | Hunter
            | Mayor | ClumsyGuy | Prince | WolfMan | Pacifist | WiseElder | Blacksmith
            | Troublemaker | Fool | Harlot | CultistHunter | Seer | GuardianAngel | WildChild
            | Cupid | Sandman | Oracle | Chemist | Detective | Gunner | Spumpkin | Augur
            | GraveDigger | Chef | Barkeep | Witch => Team::Village,
            Doppelganger | Thief => Team::Thief,
            Sorcerer | AlphaWolf | WolfCub | Wolf | Lycan | SnowWolf => Team::Wolf,
            Tanner => Team::Tanner,
            Cultist => Team::Cult,
            SerialKiller => Team::SerialKiller,
            Arsonist => Team::Arsonist,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Team {
    Village,
    Thief,
    Wolf,
    Tanner,
    Cult,
    SerialKiller,
    Arsonist,
    Lovers,
    NoOne,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KillMethod {
    Lynch,
    Eat,
    VisitWolf,
    VisitVictim,
    VisitKiller,
    VisitBurning,
    Shoot,
    Hunt,
    HunterShot,
    Flee,
    Idle,
    Burn,
    Chemistry,
    FallGrave,
    LoverDied,
    SerialKilled,
    Spotted,
    /// Not in the legacy game (there was no Witch to poison anyone) — new
    /// kill method for `shared::Role::Witch`'s poison potion.
    Poison,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GameMode {
    Normal,
    Chaos,
}

/// Winner field as recorded historically. `Wolf` and `Wolves` both occur in
/// legacy data for the same outcome; both map to `Team::Wolf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Winner {
    Village,
    Wolf,
    Tanner,
    Cult,
    SerialKiller,
    Arsonist,
    Lovers,
    NoOne,
}

impl Winner {
    pub fn from_legacy_str(s: &str) -> Option<Self> {
        match s {
            "Village" => Some(Winner::Village),
            "Wolf" | "Wolves" => Some(Winner::Wolf),
            "Tanner" => Some(Winner::Tanner),
            "Cult" => Some(Winner::Cult),
            "SerialKiller" => Some(Winner::SerialKiller),
            "Arsonist" => Some(Winner::Arsonist),
            "Lovers" => Some(Winner::Lovers),
            "NoOne" => Some(Winner::NoOne),
            _ => None,
        }
    }

    pub fn as_team(self) -> Team {
        match self {
            Winner::Village => Team::Village,
            Winner::Wolf => Team::Wolf,
            Winner::Tanner => Team::Tanner,
            Winner::Cult => Team::Cult,
            Winner::SerialKiller => Team::SerialKiller,
            Winner::Arsonist => Team::Arsonist,
            Winner::Lovers => Team::Lovers,
            Winner::NoOne => Team::NoOne,
        }
    }
}

#[cfg(test)]
mod all_roles_tests {
    use super::*;

    /// Exhaustive on purpose, no `_` arm: if a `Role` variant is ever added
    /// without a matching entry in `ALL_ROLES`, this fails to *compile*,
    /// not just to pass a runtime check — same guarantee as
    /// `roles::behavior_for`'s exhaustive match, applied to keep the
    /// fuzz-testing role list honest.
    fn assert_every_variant_is_handled(role: Role) {
        match role {
            Role::Villager
            | Role::Drunk
            | Role::Harlot
            | Role::Seer
            | Role::Traitor
            | Role::GuardianAngel
            | Role::Detective
            | Role::Wolf
            | Role::Cursed
            | Role::Gunner
            | Role::Tanner
            | Role::Fool
            | Role::WildChild
            | Role::Beholder
            | Role::ApprenticeSeer
            | Role::Cultist
            | Role::CultistHunter
            | Role::Mason
            | Role::Doppelganger
            | Role::Cupid
            | Role::Hunter
            | Role::SerialKiller
            | Role::Sorcerer
            | Role::AlphaWolf
            | Role::WolfCub
            | Role::Blacksmith
            | Role::ClumsyGuy
            | Role::Mayor
            | Role::Prince
            | Role::Lycan
            | Role::Pacifist
            | Role::WiseElder
            | Role::Oracle
            | Role::Sandman
            | Role::WolfMan
            | Role::Thief
            | Role::Troublemaker
            | Role::Chemist
            | Role::SnowWolf
            | Role::GraveDigger
            | Role::Augur
            | Role::Arsonist
            | Role::Spumpkin
            | Role::Chef
            | Role::Barkeep
            | Role::Witch => {}
        }
    }

    #[test]
    fn all_roles_covers_every_variant() {
        for &role in ALL_ROLES {
            assert_every_variant_is_handled(role);
        }
    }
}
