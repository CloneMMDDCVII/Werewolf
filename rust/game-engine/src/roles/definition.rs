//! `RoleDef`: everything the engine knows about one role, declared once in
//! that role's own file (`roles/<role>.rs`, `pub const DEF`). The engine
//! reads these facts instead of matching on `Role`, so changing a role's
//! question, its Seer appearance or its balancing weight is an edit to one
//! file.
//!
//! What lives elsewhere, on purpose:
//! - identity shared with the bot and the database (team, display name
//!   key, emoji, wolf membership) is on `shared::Role`;
//! - mechanics that involve several roles at once (who dies when the
//!   Harlot visits the wolves' victim, a cult conversion against a Hunter)
//!   stay in the regions of `night::resolve_night_sequence`, which run in
//!   the legacy order. Each role's file says which regions concern it.

use crate::roles::{PlayerId, RoleBehavior};

// What a role's `DEF` names, so a role file needs one `use`.
pub use crate::messages::Announcement;
pub use crate::orchestrator::Prompt;
pub use shared::Role;

/// One role's facts. See the module doc.
pub struct RoleDef {
    pub role: Role,
    /// Turns the player's answers into `NightAction`s and `DayAction`s.
    pub behavior: &'static dyn RoleBehavior,
    /// What the player is asked each night.
    pub night: NightQuestion,
    /// What the player is asked each day, besides the lynch vote.
    pub day: DayQuestion,
    /// The `RoleInfo…` key the player is told when roles are dealt
    /// (Werewolf.cs:1643-1711).
    pub info_key: &'static str,
    /// What a Seer checking this role is told (Werewolf.cs:3934-3953).
    pub seer_sees: SeerSees,
    /// What a Sorcerer checking this role is told.
    pub sorcerer_sees: SorcererSees,
    /// Percent chance a cult conversion of this role succeeds: the
    /// `*ConversionChance` settings (Settings.cs), 100 when there is none
    /// (Werewolf.cs:2489).
    pub cult_conversion: u32,
    /// Whether the Snow Wolf's freeze cancels this role's night action
    /// (see `freeze`).
    pub frozen_loses_action: bool,
    /// The public line for this role's death, for the deaths whose line
    /// depends on the victim's role.
    pub deaths: DeathLines,
    /// How `balancing` deals and weighs the role.
    pub balance: Balance,
    /// Whether the role can be in a game at all. Only the Witch, a role
    /// this port added, is not.
    pub dealt: bool,
}

/// The shape of a role's night question. Who a pick may name is the
/// night menu (`orchestrator::night_ask::night_options`, Werewolf.cs:5157-5212).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NightQuestion {
    None,
    /// One player from the night menu.
    Pick(Prompt),
    /// A role model, asked until one is picked (Werewolf.cs:5222-5237).
    RoleModel(Prompt),
    /// Two living players (the Cupid's lovers).
    Pair(Prompt),
    /// Douse a house not yet doused, or spark the doused ones once there
    /// are some (the Arsonist, Werewolf.cs:5280-5292).
    DouseOrSpark,
    /// Brews on the first night instead of being asked, then picks
    /// (the Chemist, Werewolf.cs:5259-5271).
    BrewThenPick(Prompt),
    /// Asked in a second batch once the wolves' target is known: heal and
    /// poison (the Witch).
    AfterWolves,
    /// Asked by `freeze::freeze_question`, in the same batch (the Snow Wolf).
    Freeze,
}

/// The shape of a role's day question.
#[derive(Debug, Clone, Copy)]
pub enum DayQuestion {
    None,
    /// One other living player.
    Pick(Prompt),
    /// The Gunner's shot, while bullets remain (Werewolf.cs:5102).
    Shoot,
    /// A yes/no ability, offered until used (Werewolf.cs:5070-5100), and
    /// announced the moment it is used (Werewolf.cs:899-973).
    Ability {
        prompt: Prompt,
        announce: fn(PlayerId) -> Announcement,
    },
}

/// What a Seer is told a role is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeerSees {
    Itself,
    As(Role),
    /// Wolf or Villager, at even odds (the Traitor).
    WolfOrVillager,
}

/// What a Sorcerer is told a role is: they only ever learn wolves, the
/// Seer and the Snow Wolf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SorcererSees {
    Wolf,
    Itself,
    Nothing,
}

/// Death lines keyed by the victim's role (`death_text`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeathLines {
    /// Eaten by the wolves.
    pub eaten: &'static str,
    /// Killed by the Serial Killer (Werewolf.cs:4251-4262).
    pub serial_killed: &'static str,
    /// Fell into a Grave Digger's grave (Werewolf.cs:4288-4302).
    pub fell_in_grave: &'static str,
    /// Killed visiting the Serial Killer (Werewolf.cs:4322-4406); `None`
    /// for a role that never visits them.
    pub visited_serial_killer: Option<&'static str>,
}

impl DeathLines {
    pub const DEFAULT: DeathLines = DeathLines {
        eaten: "DefaultEaten",
        serial_killed: "DefaultKilled",
        fell_in_grave: "DefaultFellPublic",
        visited_serial_killer: None,
    };
}

/// How `balancing` treats a role (GameBalancing.cs).
#[derive(Debug, Clone, Copy)]
pub struct Balance {
    pub group: BalanceGroup,
    /// `GetStrength` (GameBalancing.cs:216-314), given every role in the
    /// candidate game.
    pub strength: fn(&[Role]) -> i32,
    pub pool: Pool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BalanceGroup {
    Village,
    /// Village roles that reveal themselves, at most one per three
    /// players (GameBalancing.cs:44).
    RevealedVillage,
    /// Counted against the village when balancing (GameBalancing.cs:43),
    /// which is not the same split as `Team`.
    NonVillage,
}

/// Whether `balancing` puts the role in the candidate pool
/// (GameBalancing.cs:152-214).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pool {
    /// Drawn in the wolf pick, one wolf per five players.
    Wolf,
    Always,
    /// Only in games of more than ten players.
    OverTenPlayers,
    Never,
}
