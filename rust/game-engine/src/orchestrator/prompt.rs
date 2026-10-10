//! What a player is being asked ([`Prompt`]) and the legacy locale key
//! for each question.

/// Identifies *why* a player is being asked something, so a presenter can
/// look up the right prompt text (via `i18n`) without the orchestrator
/// needing to know anything about message formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Prompt {
    WolfEat,
    /// The Serial Killer's nightly kill (Werewolf.cs:5164-5168).
    SerialKillerKill,
    SeerCheck,
    HarlotVisit,
    GuardianAngelProtect,
    DetectiveInvestigate,
    FoolInvestigate,
    WildChildRoleModel,
    DoppelgangerRoleModel,
    CupidLink,
    WitchHeal,
    WitchPoison,
    /// Universal — every alive player is asked this, not just one role.
    /// See `resolve_day`.
    LynchVote,
    /// Carries the bullets left, which the legacy text shows
    /// (`AskShoot`, "{0} bullet(s) remain", Werewolf.cs:5111).
    GunnerShoot {
        bullets: u8,
    },
    BlacksmithSilver,
    CultistHunterInvestigate,
    SorcererInvestigate,
    OracleInvestigate,
    CultistConvert,
    /// Yes/no — see `Presenter::ask_toggle`.
    MayorReveal,
    /// Yes/no — see `Presenter::ask_toggle`.
    PacifistPeace,
    /// Yes/no — see `Presenter::ask_toggle`.
    SandmanSleep,
    ThiefSteal,
    ChemistBrew,
    /// Arsonist's douse target. See `ArsonistSpark` for the toggle half of
    /// the same night decision.
    ArsonistDouse,
    /// Yes/no — see `Presenter::ask_toggle`.
    ArsonistSpark,
    /// Yes/no — see `Presenter::ask_toggle`.
    TroublemakerTrouble,
    SpumpkinDetonate,
    /// Hunter's revenge shot, asked the moment they die by lynch
    /// (Werewolf.cs:5420-5441: `HunterLynchedChoice`) — see
    /// `resolve_hunter_shots`. Kept distinct from `HunterFinalShotKilled`
    /// because the legacy game asks a genuinely different question
    /// depending on `killMethod == KillMthd.Lynch`, not just cosmetic
    /// phrasing — same reasoning as never reusing another role's
    /// `Prompt` variant just because the shape matches.
    HunterFinalShotLynched,
    /// Hunter's revenge shot, asked when they die any other way
    /// (Werewolf.cs:5420-5441: `HunterShotChoice`).
    HunterFinalShotKilled,
    /// The wolves' bonus kill after a Wolf Cub dies (Werewolf.cs:1047-1053)
    /// — reuses the exact same `AskEat` text as the main wolf vote, since
    /// that's what the legacy code itself does (`GetLocaleString("AskEat")`
    /// again, not a distinct key).
    WolfCubBonusKill,
    /// Snow Wolf's nightly freeze target (Werewolf.cs:5273-5277). Built
    /// by `freeze::freeze_question`, because its options depend on who was
    /// frozen last night, then sent in the night batch.
    SnowWolfFreeze,
}

/// Maps a `Prompt` to the real `Languages/English.xml` key the legacy game
/// used for that exact menu, where one exists. Colocated with `Prompt`
/// itself (not left to whichever `Presenter` happens to need it first)
/// because "what does this identifier mean, in the legacy game's own
/// words" is a fact about the identifier, not a presentation choice — a
/// real Telegram presenter and a text-transcript presenter should both
/// read it from here rather than each maintaining their own copy that can
/// silently drift out of sync with each other. `None` means there's no
/// exact legacy key (a new role not in the original game, or a menu the
/// legacy game words differently) — callers decide
/// their own fallback wording, that part **is** a presentation choice.
///
/// Verified directly against `Languages/English.xml` and Werewolf.cs's
/// `SendNightActions`/`SendDayActions` switches,. Fool, Sorcerer and Oracle
/// reuse the Seer's `AskSee` (Werewolf.cs:5174-5178).
pub fn prompt_locale_key(prompt: Prompt) -> Option<&'static str> {
    match prompt {
        Prompt::WolfEat => Some("AskEat"),
        Prompt::SerialKillerKill => Some("AskKill"),
        Prompt::SeerCheck | Prompt::FoolInvestigate | Prompt::SorcererInvestigate | Prompt::OracleInvestigate => {
            Some("AskSee")
        }
        Prompt::HarlotVisit => Some("AskVisit"),
        Prompt::GuardianAngelProtect => Some("AskGuard"),
        Prompt::DetectiveInvestigate => Some("AskDetect"),
        Prompt::CultistHunterInvestigate => Some("AskHunt"),
        Prompt::WildChildRoleModel => Some("AskRoleModel"),
        Prompt::DoppelgangerRoleModel => Some("AskDoppelganger"),
        Prompt::CupidLink => Some("AskCupid1"),
        Prompt::LynchVote => Some("AskLynch"),
        Prompt::GunnerShoot { .. } => Some("AskShoot"),
        Prompt::CultistConvert => Some("AskConvert"),
        Prompt::MayorReveal => Some("AskMayor"),
        Prompt::PacifistPeace => Some("AskPacifist"),
        Prompt::SandmanSleep => Some("AskSandman"),
        Prompt::ThiefSteal => Some("AskThief"),
        Prompt::ChemistBrew => Some("AskChemist"),
        Prompt::ArsonistDouse | Prompt::ArsonistSpark => Some("AskArsonist"),
        Prompt::TroublemakerTrouble => Some("AskTroublemaker"),
        Prompt::HunterFinalShotLynched => Some("HunterLynchedChoice"),
        Prompt::HunterFinalShotKilled => Some("HunterShotChoice"),
        Prompt::WolfCubBonusKill => Some("AskEat"),
        Prompt::SnowWolfFreeze => Some("AskFreeze"),
        Prompt::BlacksmithSilver => Some("SpreadDust"),
        // WitchHeal/WitchPoison belong to a role that isn't in the legacy
        // game at all. SpumpkinDetonate's menu uses `AskDetonate`
        // (Werewolf.cs:5128), which English.xml doesn't have.
        Prompt::WitchHeal | Prompt::WitchPoison | Prompt::SpumpkinDetonate => None,
    }
}

/// Every `Prompt` variant, in one place, so both the completeness test
/// (`sim/tests/locale_coverage.rs`) and anything else that needs "all of
/// them" (a future exhaustive UI, say) can iterate without re-deriving the
/// list by hand and risking it drifting from the enum above.
pub const ALL_PROMPTS: &[Prompt] = &[
    Prompt::WolfEat,
    Prompt::SerialKillerKill,
    Prompt::SeerCheck,
    Prompt::HarlotVisit,
    Prompt::GuardianAngelProtect,
    Prompt::DetectiveInvestigate,
    Prompt::FoolInvestigate,
    Prompt::WildChildRoleModel,
    Prompt::DoppelgangerRoleModel,
    Prompt::CupidLink,
    Prompt::WitchHeal,
    Prompt::WitchPoison,
    Prompt::LynchVote,
    Prompt::GunnerShoot { bullets: 2 },
    Prompt::BlacksmithSilver,
    Prompt::CultistHunterInvestigate,
    Prompt::SorcererInvestigate,
    Prompt::OracleInvestigate,
    Prompt::CultistConvert,
    Prompt::MayorReveal,
    Prompt::PacifistPeace,
    Prompt::SandmanSleep,
    Prompt::ThiefSteal,
    Prompt::ChemistBrew,
    Prompt::ArsonistDouse,
    Prompt::ArsonistSpark,
    Prompt::TroublemakerTrouble,
    Prompt::SpumpkinDetonate,
    Prompt::HunterFinalShotLynched,
    Prompt::HunterFinalShotKilled,
    Prompt::WolfCubBonusKill,
    Prompt::SnowWolfFreeze,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fool_sorcerer_oracle_all_reuse_seers_exact_prompt() {
        assert_eq!(prompt_locale_key(Prompt::SeerCheck), Some("AskSee"));
        assert_eq!(prompt_locale_key(Prompt::FoolInvestigate), Some("AskSee"));
        assert_eq!(prompt_locale_key(Prompt::SorcererInvestigate), Some("AskSee"));
        assert_eq!(prompt_locale_key(Prompt::OracleInvestigate), Some("AskSee"));
    }

    #[test]
    fn a_new_role_has_no_legacy_prompt_key() {
        assert_eq!(prompt_locale_key(Prompt::WitchHeal), None);
    }
}
