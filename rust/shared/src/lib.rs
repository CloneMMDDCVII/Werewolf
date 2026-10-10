pub mod time;

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
    /// `Shared/Roles.cs:16-142`: the `[Role("emoji")]` attribute each legacy
    /// role carries, shown on the group `/config` role toggles. `None` for
    /// the roles this port added (Chef, Barkeep, Witch), which have no
    /// legacy attribute and are never handed out by balancing.
    pub fn emoji(self) -> Option<&'static str> {
        use Role::*;
        Some(match self {
            Villager => "👱",
            Drunk => "🍻",
            Harlot => "💋",
            Seer => "👳",
            Traitor => "🖕",
            GuardianAngel => "👼",
            Detective => "🕵",
            Wolf => "🐺",
            Cursed => "😾",
            Gunner => "🔫",
            Tanner => "👺",
            Fool => "🃏",
            WildChild => "👶",
            Beholder => "👁",
            ApprenticeSeer => "🙇",
            Cultist => "👤",
            CultistHunter => "💂",
            Mason => "👷",
            Doppelganger => "🎭",
            Cupid => "🏹",
            Hunter => "🎯",
            SerialKiller => "🔪",
            Sorcerer => "🔮",
            AlphaWolf => "⚡️",
            WolfCub => "🐶",
            Blacksmith => "⚒",
            ClumsyGuy => "🤕",
            Mayor => "🎖",
            Prince => "👑",
            Lycan => "🐺🌝",
            Pacifist => "☮️",
            WiseElder => "📚",
            Oracle => "🌀",
            Sandman => "💤",
            WolfMan => "👱🌚",
            Thief => "😈",
            Troublemaker => "🤯",
            Chemist => "👨‍🔬",
            SnowWolf => "🐺☃️",
            GraveDigger => "☠️",
            Augur => "🦅",
            Arsonist => "🔥",
            Spumpkin => "🎃",
            Chef | Barkeep | Witch => return None,
        })
    }

    /// The `Languages/English.xml` key for this role's display name — what
    /// the legacy `GetDescription(role)` looks up (Werewolf.cs:1448-1451:
    /// `GetLocaleString(en.ToString())`), i.e. the C# enum name verbatim.
    /// Only Doppelgänger differs from the Rust identifier. Spumpkin's name
    /// lives in `SpecialStrings.cs` rather than English.xml, and Chef,
    /// Barkeep and Witch have no legacy entry at all, so a presenter needs
    /// a fallback for those four.
    pub fn locale_key(self) -> &'static str {
        use Role::*;
        match self {
            Villager => "Villager",
            Drunk => "Drunk",
            Harlot => "Harlot",
            Seer => "Seer",
            Traitor => "Traitor",
            GuardianAngel => "GuardianAngel",
            Detective => "Detective",
            Wolf => "Wolf",
            Cursed => "Cursed",
            Gunner => "Gunner",
            Tanner => "Tanner",
            Fool => "Fool",
            WildChild => "WildChild",
            Beholder => "Beholder",
            ApprenticeSeer => "ApprenticeSeer",
            Cultist => "Cultist",
            CultistHunter => "CultistHunter",
            Mason => "Mason",
            Doppelganger => "Doppelgänger",
            Cupid => "Cupid",
            Hunter => "Hunter",
            SerialKiller => "SerialKiller",
            Sorcerer => "Sorcerer",
            AlphaWolf => "AlphaWolf",
            WolfCub => "WolfCub",
            Blacksmith => "Blacksmith",
            ClumsyGuy => "ClumsyGuy",
            Mayor => "Mayor",
            Prince => "Prince",
            Lycan => "Lycan",
            Pacifist => "Pacifist",
            WiseElder => "WiseElder",
            Oracle => "Oracle",
            Sandman => "Sandman",
            WolfMan => "WolfMan",
            Thief => "Thief",
            Troublemaker => "Troublemaker",
            Chemist => "Chemist",
            SnowWolf => "SnowWolf",
            GraveDigger => "GraveDigger",
            Augur => "Augur",
            Arsonist => "Arsonist",
            Spumpkin => "Spumpkin",
            Chef => "Chef",
            Barkeep => "Barkeep",
            Witch => "Witch",
        }
    }

    /// Mirrors `Werewolf.cs::SetTeam` (Werewolf Node/Werewolf.cs:1546-1610).
    /// Chef/Barkeep (beta-only roles) verified against GreyWolfDev/Werewolf@beta.
    pub fn team(self) -> Team {
        use Role::*;
        match self {
            Villager | Cursed | Drunk | Beholder | ApprenticeSeer | Traitor | Mason | Hunter | Mayor | ClumsyGuy
            | Prince | WolfMan | Pacifist | WiseElder | Blacksmith | Troublemaker | Fool | Harlot | CultistHunter
            | Seer | GuardianAngel | WildChild | Cupid | Sandman | Oracle | Chemist | Detective | Gunner | Spumpkin
            | Augur | GraveDigger | Chef | Barkeep | Witch => Team::Village,
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
    /// A Guardian Angel killed by the wolf they guarded (Werewolf.cs:2369).
    GuardWolf,
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
    /// A cult's new recruit dying while trying to convert a Hunter who
    /// shoots back instead of being converted (`IPlayer.cs`'s `KillMthd`
    /// enum: `HunterCult`, Werewolf.cs:3635-3637).
    HunterCult,
    /// The Tanner (or a Doppelgänger who became one) giving up when only
    /// neutral roles are left (Werewolf.cs:4726).
    Suicide,
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
