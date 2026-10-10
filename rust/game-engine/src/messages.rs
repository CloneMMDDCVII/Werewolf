//! Text the legacy game sends that is neither a question (`Prompt`) nor a
//! death or transform: private results told to one player (a Seer's
//! reading, a freeze notice) and public announcements to the whole group
//! (a tied lynch). Each variant maps to exactly one `Languages/English.xml`
//! key and carries the data that key's placeholders need, in order, so a
//! presenter can render it without reaching back into game state — the
//! same contract `NarrationEvent::Death` already follows.

use crate::roles::PlayerId;
use shared::Role;

/// One placeholder value. A presenter renders `Player` as that player's
/// display name and `Role` as the role's display name (`Role::locale_key`,
/// the legacy `GetDescription`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageArg {
    Player(PlayerId),
    Role(Role),
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Announcement {
    /// Werewolf.cs:2785.
    LynchTie,
    /// Werewolf.cs:2792.
    NoLynchVotes,
    /// Werewolf.cs:972, the moment the Troublemaker acts.
    TroublemakerDoubleLynch { troublemaker: PlayerId },
    /// Werewolf.cs:2566, right before the second lynch vote.
    TroubleDoubleLynchNow,
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
        }
    }

    pub fn args(&self) -> Vec<MessageArg> {
        match *self {
            Announcement::TroublemakerDoubleLynch { troublemaker } => vec![MessageArg::Player(troublemaker)],
            Announcement::LynchTie | Announcement::NoLynchVotes | Announcement::TroubleDoubleLynchNow => vec![],
        }
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
        ]
        .iter()
        .map(Announcement::locale_key),
    );
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
    fn message_keys_are_distinct() {
        let keys = all_message_keys();
        let mut sorted = keys.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len());
    }
}
