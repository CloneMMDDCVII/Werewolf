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
}

impl Role {
    /// Mirrors `Werewolf.cs::SetTeam` (Werewolf Node/Werewolf.cs:1546-1610).
    pub fn team(self) -> Team {
        use Role::*;
        match self {
            Villager | Cursed | Drunk | Beholder | ApprenticeSeer | Traitor | Mason | Hunter
            | Mayor | ClumsyGuy | Prince | WolfMan | Pacifist | WiseElder | Blacksmith
            | Troublemaker | Fool | Harlot | CultistHunter | Seer | GuardianAngel | WildChild
            | Cupid | Sandman | Oracle | Chemist | Detective | Gunner | Spumpkin | Augur
            | GraveDigger | Chef | Barkeep => Team::Village,
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
}
