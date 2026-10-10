//! Text the legacy game sends that is neither a question (`Prompt`) nor a
//! death or transform: private results told to one player (a Seer's
//! reading, a freeze notice) and public announcements to the whole group
//! (a tied lynch). Each variant maps to exactly one `Languages/English.xml`
//! key and carries the data that key's placeholders need, in order, so a
//! presenter can render it without reaching back into game state — the
//! same contract `NarrationEvent::Death` already follows.
//!
//! `render` turns a `Message` into text given a key lookup and player
//! names, so every presenter fills templates the same way.

use crate::roles::PlayerId;
use shared::Role;

/// A locale key plus its placeholder values, in `{0}`, `{1}`, ... order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub key: &'static str,
    pub args: Vec<MessageArg>,
    /// A second sentence sent in the same message, after a space (for
    /// example `HunterShotWolf` followed by `PlayerRoleWas`).
    pub then: Option<Box<Message>>,
}

impl Message {
    pub fn new(key: &'static str, args: Vec<MessageArg>) -> Self {
        Message { key, args, then: None }
    }

    pub fn followed_by(mut self, next: Message) -> Self {
        self.then = Some(Box::new(next));
        self
    }
}

/// One placeholder value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageArg {
    /// Rendered as the player's display name.
    Player(PlayerId),
    /// Rendered as the role's display name (`Role::locale_key`, the
    /// legacy `GetDescription`).
    Role(Role),
    Number(i64),
    /// Voter names, comma-separated, with a count when someone's vote
    /// counted more than once ("Ann, Bob (2)", Werewolf.cs:2728).
    Voters(Vec<(PlayerId, u32)>),
    /// Nested messages, one per line.
    Lines(Vec<Message>),
    /// "{name} was {role}", the reveal legacy death lines append when the
    /// group shows roles on death (`$"{p.GetName()} {GetLocaleString("Was")}
    /// {GetDescription(p.PlayerRole)}"`).
    Reveal { player: PlayerId, role: Role },
    /// "{role} is dead" (Werewolf.cs:4348).
    RoleIsDead(Role),
    /// Nothing: a reveal left out because roles are hidden.
    Empty,
}

/// Fills `message`'s template. `lookup` returns the text for a locale key;
/// a missing key renders as `(Key)` so a gap is visible, and a role with
/// no display-name entry falls back to its enum name. Literal `\n` in a
/// template becomes a newline, as the legacy `GetLocaleString` does.
pub fn render(message: &Message, lookup: &dyn Fn(&str) -> Option<String>, name: &dyn Fn(PlayerId) -> String) -> String {
    let role_name = |role: Role| lookup(role.locale_key()).unwrap_or_else(|| format!("{role:?}"));
    let mut text = lookup(message.key).unwrap_or_else(|| format!("({})", message.key)).replace("\\n", "\n");
    for (i, arg) in message.args.iter().enumerate() {
        let value = match arg {
            MessageArg::Player(id) => name(*id),
            MessageArg::Role(role) => role_name(*role),
            MessageArg::Number(n) => n.to_string(),
            MessageArg::Voters(voters) => voters
                .iter()
                .map(|&(id, n)| if n > 1 { format!("{} ({n})", name(id)) } else { name(id) })
                .collect::<Vec<_>>()
                .join(", "),
            MessageArg::Lines(lines) => {
                lines.iter().map(|m| render(m, lookup, name)).collect::<Vec<_>>().join("\n")
            }
            MessageArg::Reveal { player, role } => {
                let was = lookup("Was").unwrap_or_else(|| "was".to_string());
                format!("{} {was} {}", name(*player), role_name(*role))
            }
            MessageArg::RoleIsDead(role) => {
                format!("{} {}", role_name(*role), lookup("IsDead").unwrap_or_else(|| "is dead".to_string()))
            }
            MessageArg::Empty => String::new(),
        };
        text = text.replace(&format!("{{{i}}}"), &value);
    }
    match &message.then {
        Some(next) => format!("{text} {}", render(next, lookup, name)),
        None => text,
    }
}

/// Something told privately to one player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note {
    /// Seer, Sorcerer and Fool readings all use this key
    /// (Werewolf.cs:3954, 3970-3976, 4016). `role` is what the reader is
    /// *told*, which is often not the target's real role.
    SeerSees { target: PlayerId, role: Role },
    /// Sorcerer looking at anyone who is not a wolf or a seer
    /// (Werewolf.cs:3979).
    SorcererOther { target: PlayerId },
    /// Oracle: a role the target is not (Werewolf.cs:4032).
    NegSeerSees { target: PlayerId, role: Role },
    /// Augur: a role nobody alive holds (Werewolf.cs:4054).
    AugurSees { role: Role },
    /// Augur, once every absent role has been shown (Werewolf.cs:4059).
    AugurSeesNothing,
    /// Detective's daytime snoop shows the exact role (Werewolf.cs:2945).
    DetectiveSnoop { target: PlayerId, role: Role },
    /// Told to every wolf when the Detective is caught (Werewolf.cs:2942).
    DetectiveCaught { detective: PlayerId },
    /// Told to a player the Snow Wolf froze (Werewolf.cs:3104-3176).
    Frozen(FrozenNotice),
    /// Told to the Snow Wolf after a freeze lands (Werewolf.cs:3105, 3118, 3179).
    SuccessfulFreeze { target: PlayerId },
    /// Told to the Snow Wolf when a Guardian Angel blocks the freeze
    /// (Werewolf.cs:3110).
    GuardBlockedSnowWolf { target: PlayerId },
    /// Full-mode Thief (Werewolf.cs:4171-4186).
    ThiefStealFailed { target: PlayerId },
    ThiefStealDead { target: PlayerId },
    ThiefFell { target: PlayerId },
    StealKiller,
    /// Told to the Thief after a steal (Werewolf.cs:2214).
    ThiefStoleRole { target: PlayerId },
    /// Told to the player robbed by a Thief (Werewolf.cs:2261): they
    /// become the Thief in full mode, a Villager otherwise.
    ThiefStoleYourRoleThief,
    ThiefStoleYourRoleVillager,
    /// Told to the Grave Digger whose trap caught a visitor
    /// (Werewolf.cs:2394-2412).
    FellDigger { visitor: PlayerId, by: DiggerVisitor },
    /// Cultist Hunter (Werewolf.cs:3569-3584).
    HunterFindCultist { target: PlayerId },
    CHHuntedYou,
    HunterFailedToFind { target: PlayerId },
    HunterVisitDead { target: PlayerId },
}

/// Which flavour of freeze notice the frozen player gets — the legacy
/// switch picks one per role (Werewolf.cs:3128-3177). None take arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrozenNotice {
    Default,
    Harlot,
    Chemist,
    Cultist,
    CultistHunter,
    /// Seer, Sorcerer, Fool, Oracle and Augur share one key.
    Seeing,
    GuardianAngel,
    SerialKiller,
    GraveDigger,
    /// The Arsonist shrugs the freeze off but is still told about it.
    ArsonistNotFrozen,
    /// A full-mode Thief, who loses tonight's steal.
    Thief,
}

/// Who fell into the Grave Digger's trap, which picks the Grave Digger's
/// notice (Werewolf.cs:2394-2412).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiggerVisitor {
    Wolf,
    CultistHunter,
    Cultist,
    GuardianAngel,
    Harlot,
    Thief,
}

/// Something announced to the whole group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Announcement {
    /// Werewolf.cs:2785.
    LynchTie,
    /// Werewolf.cs:2792.
    NoLynchVotes,
    /// Werewolf.cs:972, the moment the Troublemaker acts.
    TroublemakerDoubleLynch { troublemaker: PlayerId },
    /// Werewolf.cs:2566, right before the second lynch vote.
    TroubleDoubleLynchNow,
    /// Werewolf.cs:3028.
    NightTime { seconds: u32 },
    /// Werewolf.cs:2829.
    DayTime { seconds: u32 },
    /// Werewolf.cs:2830.
    Day { number: u32 },
    /// Werewolf.cs:2567.
    LynchTime { seconds: u32 },
    /// A public lynch vote (Werewolf.cs:1129).
    PlayerVotedLynch { voter: PlayerId, target: PlayerId },
    /// A secret lynch vote: only the running count (Werewolf.cs:1134).
    PlayerVoteCounts { voted: u32, alive: u32 },
    /// Secret lynch results after the vote, most votes first
    /// (Werewolf.cs:2717-2741). `voters` is set only when the group shows
    /// voters.
    SecretLynchResults(Vec<SecretTally>),
    /// Werewolf.cs:901, when the Mayor reveals.
    MayorReveal { mayor: PlayerId },
    /// Werewolf.cs:918, when the Pacifist calls for peace.
    PacifistNoLynch { pacifist: PlayerId },
    /// Werewolf.cs:2561, in place of the lynch the Pacifist stopped.
    PacifistNoLynchNow,
    /// Werewolf.cs:941, when the Blacksmith spreads silver.
    BlacksmithSpreadSilver { blacksmith: PlayerId },
    /// Werewolf.cs:2747, the Prince surviving their first lynch.
    PrinceLynched { prince: PlayerId },
    /// Werewolf.cs:5462, a dying Hunter who shoots nobody. `lynched` is
    /// how the Hunter died.
    HunterSkipChoice { hunter: PlayerId, lynched: bool },
    /// Werewolf.cs:5473, a dying Hunter who shot the Wise Elder.
    HunterKilledWiseElder { hunter: PlayerId, elder: PlayerId },
    /// Last-two endings (Werewolf.cs:4544-4578, 4840-4894).
    HunterKillsWolfEnd { hunter: PlayerId, wolf: PlayerId },
    WolfKillsHunterEnd { hunter: PlayerId, wolf: PlayerId },
    SKHunterEnd { serial_killer: PlayerId, hunter: PlayerId },
    CHKillsCultistEnd { cultist: PlayerId, hunter: PlayerId },
    SerialKillerWinsOverpower { serial_killer: PlayerId, other: PlayerId },
    ArsonistWinsOverpower { arsonist: PlayerId, other: PlayerId },
    /// A neutral role left at a no-winner ending: `SorcererEnd`,
    /// `ThiefEnd`, `DoppelgangerEnd` or `TannerEnd` (Werewolf.cs:4696-4806).
    NeutralEnd { player: PlayerId, role: Role },
}

/// One line of the secret lynch results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretTally {
    pub target: PlayerId,
    pub votes: u32,
    pub voters: Option<Vec<(PlayerId, u32)>>,
}

impl Note {
    pub fn locale_key(&self) -> &'static str {
        match self {
            Note::SeerSees { .. } => "SeerSees",
            Note::SorcererOther { .. } => "SorcererOther",
            Note::NegSeerSees { .. } => "NegSeerSees",
            Note::AugurSees { .. } => "AugurSees",
            Note::AugurSeesNothing => "AugurSeesNothing",
            Note::DetectiveSnoop { .. } => "DetectiveSnoop",
            Note::DetectiveCaught { .. } => "DetectiveCaught",
            Note::Frozen(notice) => match notice {
                FrozenNotice::Default => "DefaultFrozen",
                FrozenNotice::Harlot => "HarlotFrozen",
                FrozenNotice::Chemist => "ChemistFrozen",
                FrozenNotice::Cultist => "CultistFrozen",
                FrozenNotice::CultistHunter => "CHFrozen",
                FrozenNotice::Seeing => "SeeingFrozen",
                FrozenNotice::GuardianAngel => "GAFrozen",
                FrozenNotice::SerialKiller => "SKFrozen",
                FrozenNotice::GraveDigger => "GraveDiggerFrozen",
                FrozenNotice::ArsonistNotFrozen => "ArsonistNotFrozen",
                FrozenNotice::Thief => "ThiefFrozen",
            },
            Note::SuccessfulFreeze { .. } => "SuccessfulFreeze",
            Note::GuardBlockedSnowWolf { .. } => "GuardBlockedSnowWolf",
            Note::ThiefStealFailed { .. } => "ThiefStealFailed",
            Note::ThiefStealDead { .. } => "ThiefStealDead",
            Note::ThiefFell { .. } => "ThiefFell",
            Note::StealKiller => "StealKiller",
            Note::ThiefStoleRole { .. } => "ThiefStoleRole",
            Note::ThiefStoleYourRoleThief => "ThiefStoleYourRoleThief",
            Note::ThiefStoleYourRoleVillager => "ThiefStoleYourRoleVillager",
            Note::FellDigger { by, .. } => match by {
                DiggerVisitor::Wolf => "WolfFell",
                DiggerVisitor::CultistHunter => "HunterFellDigger",
                DiggerVisitor::Cultist => "CultFell",
                DiggerVisitor::GuardianAngel => "GAFellDigger",
                DiggerVisitor::Harlot => "HarlotFellDigger",
                DiggerVisitor::Thief => "ThiefFellDigger",
            },
            Note::HunterFindCultist { .. } => "HunterFindCultist",
            Note::CHHuntedYou => "CHHuntedYou",
            Note::HunterFailedToFind { .. } => "HunterFailedToFind",
            Note::HunterVisitDead { .. } => "HunterVisitDead",
        }
    }

    pub fn message(&self) -> Message {
        Message::new(self.locale_key(), self.args())
    }

    /// Placeholder values in `{0}`, `{1}`, ... order.
    pub fn args(&self) -> Vec<MessageArg> {
        use MessageArg::{Player, Role as R};
        match *self {
            Note::SeerSees { target, role }
            | Note::NegSeerSees { target, role }
            | Note::DetectiveSnoop { target, role } => vec![Player(target), R(role)],
            Note::SorcererOther { target }
            | Note::SuccessfulFreeze { target }
            | Note::GuardBlockedSnowWolf { target }
            | Note::ThiefStealFailed { target }
            | Note::ThiefStealDead { target }
            | Note::ThiefFell { target }
            | Note::ThiefStoleRole { target }
            | Note::HunterFindCultist { target }
            | Note::HunterFailedToFind { target }
            | Note::HunterVisitDead { target } => vec![Player(target)],
            Note::FellDigger { visitor, .. } => vec![Player(visitor)],
            Note::AugurSees { role } => vec![R(role)],
            Note::DetectiveCaught { detective } => vec![Player(detective)],
            Note::AugurSeesNothing
            | Note::Frozen(_)
            | Note::StealKiller
            | Note::ThiefStoleYourRoleThief
            | Note::ThiefStoleYourRoleVillager
            | Note::CHHuntedYou => vec![],
        }
    }
}

impl Announcement {
    pub fn locale_key(&self) -> &'static str {
        match self {
            Announcement::LynchTie => "LynchTie",
            Announcement::NoLynchVotes => "NoLynchVotes",
            Announcement::TroublemakerDoubleLynch { .. } => "TroublemakerDoubleLynch",
            Announcement::TroubleDoubleLynchNow => "TroubleDoubleLynchNow",
            Announcement::NightTime { .. } => "NightTime",
            Announcement::DayTime { .. } => "DayTime",
            Announcement::Day { .. } => "Day",
            Announcement::LynchTime { .. } => "LynchTime",
            Announcement::PlayerVotedLynch { .. } => "PlayerVotedLynch",
            Announcement::PlayerVoteCounts { .. } => "PlayerVoteCounts",
            Announcement::SecretLynchResults(_) => "SecretLynchResultFull",
            Announcement::MayorReveal { .. } => "MayorReveal",
            Announcement::PacifistNoLynch { .. } => "PacifistNoLynch",
            Announcement::PacifistNoLynchNow => "PacifistNoLynchNow",
            Announcement::BlacksmithSpreadSilver { .. } => "BlacksmithSpreadSilver",
            Announcement::PrinceLynched { .. } => "PrinceLynched",
            Announcement::HunterSkipChoice { lynched: true, .. } => "HunterSkipChoiceLynched",
            Announcement::HunterSkipChoice { lynched: false, .. } => "HunterSkipChoiceShot",
            Announcement::HunterKilledWiseElder { .. } => "HunterKilledWiseElder",
            Announcement::HunterKillsWolfEnd { .. } => "HunterKillsWolfEnd",
            Announcement::WolfKillsHunterEnd { .. } => "WolfKillsHunterEnd",
            Announcement::SKHunterEnd { .. } => "SKHunterEnd",
            Announcement::CHKillsCultistEnd { .. } => "CHKillsCultistEnd",
            Announcement::SerialKillerWinsOverpower { .. } => "SerialKillerWinsOverpower",
            Announcement::ArsonistWinsOverpower { .. } => "ArsonistWinsOverpower",
            Announcement::NeutralEnd { role, .. } => match role {
                Role::Sorcerer => "SorcererEnd",
                Role::Thief => "ThiefEnd",
                Role::Doppelganger => "DoppelgangerEnd",
                _ => "TannerEnd",
            },
        }
    }

    pub fn args(&self) -> Vec<MessageArg> {
        use MessageArg::{Number, Player};
        match self {
            Announcement::TroublemakerDoubleLynch { troublemaker: one }
            | Announcement::MayorReveal { mayor: one }
            | Announcement::PacifistNoLynch { pacifist: one }
            | Announcement::BlacksmithSpreadSilver { blacksmith: one }
            | Announcement::PrinceLynched { prince: one }
            | Announcement::HunterSkipChoice { hunter: one, .. } => vec![Player(*one)],
            Announcement::HunterKilledWiseElder { hunter: a, elder: b }
            | Announcement::HunterKillsWolfEnd { hunter: a, wolf: b }
            | Announcement::WolfKillsHunterEnd { hunter: a, wolf: b }
            | Announcement::SKHunterEnd { serial_killer: a, hunter: b }
            | Announcement::CHKillsCultistEnd { cultist: a, hunter: b }
            | Announcement::SerialKillerWinsOverpower { serial_killer: a, other: b }
            | Announcement::ArsonistWinsOverpower { arsonist: a, other: b } => vec![Player(*a), Player(*b)],
            Announcement::NeutralEnd { player, .. } => vec![Player(*player)],
            Announcement::NightTime { seconds } | Announcement::DayTime { seconds } | Announcement::LynchTime { seconds } => {
                vec![Number(*seconds as i64)]
            }
            Announcement::Day { number } => vec![Number(*number as i64)],
            Announcement::PlayerVotedLynch { voter, target } => vec![Player(*voter), Player(*target)],
            Announcement::PlayerVoteCounts { voted, alive } => vec![Number(*voted as i64), Number(*alive as i64)],
            Announcement::SecretLynchResults(tallies) => vec![MessageArg::Lines(
                tallies
                    .iter()
                    .map(|t| match &t.voters {
                        Some(voters) => Message::new(
                            "SecretLynchResultEach",
                            vec![Number(t.votes as i64), Player(t.target), MessageArg::Voters(voters.clone())],
                        ),
                        None => Message::new("SecretLynchResultNumber", vec![Number(t.votes as i64), Player(t.target)]),
                    })
                    .collect(),
            )],
            Announcement::LynchTie
            | Announcement::NoLynchVotes
            | Announcement::TroubleDoubleLynchNow
            | Announcement::PacifistNoLynchNow => vec![],
        }
    }

    pub fn message(&self) -> Message {
        Message::new(self.locale_key(), self.args())
    }
}

/// Every key a `Note` or `Announcement` can produce, for the locale
/// coverage check. Built from one sample of each variant rather than a
/// hand-typed string list, so a typo in `locale_key` shows up there too.
pub fn all_message_keys() -> Vec<&'static str> {
    let p = PlayerId(0);
    let notices = [
        FrozenNotice::Default,
        FrozenNotice::Harlot,
        FrozenNotice::Chemist,
        FrozenNotice::Cultist,
        FrozenNotice::CultistHunter,
        FrozenNotice::Seeing,
        FrozenNotice::GuardianAngel,
        FrozenNotice::SerialKiller,
        FrozenNotice::GraveDigger,
        FrozenNotice::ArsonistNotFrozen,
        FrozenNotice::Thief,
    ];
    let mut keys: Vec<&'static str> = [
        Note::SeerSees { target: p, role: Role::Wolf },
        Note::SorcererOther { target: p },
        Note::NegSeerSees { target: p, role: Role::Wolf },
        Note::AugurSees { role: Role::Wolf },
        Note::AugurSeesNothing,
        Note::DetectiveSnoop { target: p, role: Role::Wolf },
        Note::DetectiveCaught { detective: p },
        Note::SuccessfulFreeze { target: p },
        Note::GuardBlockedSnowWolf { target: p },
        Note::ThiefStealFailed { target: p },
        Note::ThiefStealDead { target: p },
        Note::ThiefFell { target: p },
        Note::StealKiller,
        Note::ThiefStoleRole { target: p },
        Note::ThiefStoleYourRoleThief,
        Note::ThiefStoleYourRoleVillager,
        Note::FellDigger { visitor: p, by: DiggerVisitor::Wolf },
        Note::FellDigger { visitor: p, by: DiggerVisitor::CultistHunter },
        Note::FellDigger { visitor: p, by: DiggerVisitor::Cultist },
        Note::FellDigger { visitor: p, by: DiggerVisitor::GuardianAngel },
        Note::FellDigger { visitor: p, by: DiggerVisitor::Harlot },
        Note::FellDigger { visitor: p, by: DiggerVisitor::Thief },
        Note::HunterFindCultist { target: p },
        Note::CHHuntedYou,
        Note::HunterFailedToFind { target: p },
        Note::HunterVisitDead { target: p },
    ]
    .iter()
    .chain(notices.iter().map(|&n| Note::Frozen(n)).collect::<Vec<_>>().iter())
    .map(Note::locale_key)
    .collect();
    keys.extend(
        [
            Announcement::LynchTie,
            Announcement::NoLynchVotes,
            Announcement::TroublemakerDoubleLynch { troublemaker: p },
            Announcement::TroubleDoubleLynchNow,
            Announcement::NightTime { seconds: 0 },
            Announcement::DayTime { seconds: 0 },
            Announcement::Day { number: 0 },
            Announcement::LynchTime { seconds: 0 },
            Announcement::PlayerVotedLynch { voter: p, target: p },
            Announcement::PlayerVoteCounts { voted: 0, alive: 0 },
            Announcement::SecretLynchResults(vec![]),
            Announcement::MayorReveal { mayor: p },
            Announcement::PacifistNoLynch { pacifist: p },
            Announcement::PacifistNoLynchNow,
            Announcement::BlacksmithSpreadSilver { blacksmith: p },
            Announcement::PrinceLynched { prince: p },
            Announcement::HunterSkipChoice { hunter: p, lynched: true },
            Announcement::HunterSkipChoice { hunter: p, lynched: false },
            Announcement::HunterKilledWiseElder { hunter: p, elder: p },
            Announcement::HunterKillsWolfEnd { hunter: p, wolf: p },
            Announcement::WolfKillsHunterEnd { hunter: p, wolf: p },
            Announcement::SKHunterEnd { serial_killer: p, hunter: p },
            Announcement::CHKillsCultistEnd { cultist: p, hunter: p },
            Announcement::SerialKillerWinsOverpower { serial_killer: p, other: p },
            Announcement::ArsonistWinsOverpower { arsonist: p, other: p },
            Announcement::NeutralEnd { player: p, role: Role::Sorcerer },
            Announcement::NeutralEnd { player: p, role: Role::Thief },
            Announcement::NeutralEnd { player: p, role: Role::Doppelganger },
            Announcement::NeutralEnd { player: p, role: Role::Tanner },
        ]
        .iter()
        .map(Announcement::locale_key),
    );
    keys.extend(["SecretLynchResultEach", "SecretLynchResultNumber"]);
    keys.extend(crate::summary::SUMMARY_KEYS);
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seer_reading_names_the_target_then_the_role() {
        let note = Note::SeerSees { target: PlayerId(3), role: Role::Wolf };
        assert_eq!(note.locale_key(), "SeerSees");
        assert_eq!(note.args(), vec![MessageArg::Player(PlayerId(3)), MessageArg::Role(Role::Wolf)]);
    }

    #[test]
    fn every_seeing_role_shares_one_freeze_key() {
        assert_eq!(Note::Frozen(FrozenNotice::Seeing).locale_key(), "SeeingFrozen");
        assert_eq!(Note::Frozen(FrozenNotice::CultistHunter).locale_key(), "CHFrozen");
    }

    #[test]
    fn render_fills_players_roles_numbers_and_nested_lines() {
        let lookup = |key: &str| {
            Some(
                match key {
                    "SecretLynchResultFull" => "Result:\\n{0}",
                    "SecretLynchResultEach" => "{0} votes went to {1}: {2}",
                    "SeerSees" => "{0} is {1}",
                    "Wolf" => "a Werewolf",
                    _ => return None,
                }
                .to_string(),
            )
        };
        let name = |id: PlayerId| format!("P{}", id.0);
        let results = Announcement::SecretLynchResults(vec![SecretTally {
            target: PlayerId(2),
            votes: 3,
            voters: Some(vec![(PlayerId(1), 2), (PlayerId(3), 1)]),
        }]);
        assert_eq!(render(&results.message(), &lookup, &name), "Result:\n3 votes went to P2: P1 (2), P3");
        let seer = Note::SeerSees { target: PlayerId(4), role: Role::Wolf };
        assert_eq!(render(&seer.message(), &lookup, &name), "P4 is a Werewolf");
        assert_eq!(render(&Message::new("Nope", vec![]), &lookup, &name), "(Nope)");
    }

    #[test]
    fn message_keys_are_distinct() {
        let keys = all_message_keys();
        let mut sorted = keys.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len());
    }
}
