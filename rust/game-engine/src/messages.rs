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
}

impl Message {
    pub fn new(key: &'static str, args: Vec<MessageArg>) -> Self {
        Message { key, args }
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
}

/// Fills `message`'s template. `lookup` returns the text for a locale key;
/// a missing key renders as `(Key)` so a gap is visible, and a role with
/// no display-name entry falls back to its enum name. Literal `\n` in a
/// template becomes a newline, as the legacy `GetLocaleString` does.
pub fn render(message: &Message, lookup: &dyn Fn(&str) -> Option<String>, name: &dyn Fn(PlayerId) -> String) -> String {
    let mut text = lookup(message.key).unwrap_or_else(|| format!("({})", message.key)).replace("\\n", "\n");
    for (i, arg) in message.args.iter().enumerate() {
        let value = match arg {
            MessageArg::Player(id) => name(*id),
            MessageArg::Role(role) => lookup(role.locale_key()).unwrap_or_else(|| format!("{role:?}")),
            MessageArg::Number(n) => n.to_string(),
            MessageArg::Voters(voters) => voters
                .iter()
                .map(|&(id, n)| if n > 1 { format!("{} ({n})", name(id)) } else { name(id) })
                .collect::<Vec<_>>()
                .join(", "),
            MessageArg::Lines(lines) => {
                lines.iter().map(|m| render(m, lookup, name)).collect::<Vec<_>>().join("\n")
            }
        };
        text = text.replace(&format!("{{{i}}}"), &value);
    }
    text
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
            },
            Note::SuccessfulFreeze { .. } => "SuccessfulFreeze",
            Note::GuardBlockedSnowWolf { .. } => "GuardBlockedSnowWolf",
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
            | Note::GuardBlockedSnowWolf { target } => vec![Player(target)],
            Note::AugurSees { role } => vec![R(role)],
            Note::DetectiveCaught { detective } => vec![Player(detective)],
            Note::AugurSeesNothing | Note::Frozen(_) => vec![],
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
        }
    }

    pub fn args(&self) -> Vec<MessageArg> {
        use MessageArg::{Number, Player};
        match self {
            Announcement::TroublemakerDoubleLynch { troublemaker } => vec![Player(*troublemaker)],
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
            Announcement::LynchTie | Announcement::NoLynchVotes | Announcement::TroubleDoubleLynchNow => vec![],
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
