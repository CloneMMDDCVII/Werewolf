//! Everything that asks players something, and the day. Questions go out
//! through the [`Presenter`] trait, so this module never knows whether it
//! is talking to Telegram or a scripted test. `bot` and `sim` each
//! implement `Presenter` their own way.
//!
//! `Presenter` asks for targets with a count (`ask_targets(.., count)`)
//! rather than one method per number of targets, so a role that picks
//! three players needs no new method on every presenter.
//!
//! Which question each role gets is its `RoleDef` (`roles::def`). The
//! night is asked in one batch, then a second for roles that need the
//! wolves' target first (`NightQuestion::AfterWolves`, the Witch).

use crate::death_text::{death_message, DeathContext};
use crate::messages::{Announcement, Message, MessageArg, Note, SecretTally};
use crate::rng::RngSource;
use crate::roles::definition::{DayQuestion, NightQuestion};
use crate::roles::{def, DayAction, DayContext, LoversState, NightAction, NightContext, PlayerId, RoleState};
use crate::rules::GameRules;
use async_trait::async_trait;
use shared::{KillMethod, Role, Team};
use std::collections::HashMap;

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

/// Something that happened, worth narrating, that *isn't* a question —
/// the counterpart to `Prompt` for events instead of decisions. Carries
/// whatever a presenter needs to pick the right legacy flavor text
/// itself (the victim's role *at the moment they died*, not just their
/// id) — a presenter should never need to reach back into game state to
/// answer "what do I say," the event handed to it should already be
/// self-contained, the same way `NightContext`/`DayContext` hand a role
/// everything it needs instead of letting it query other players' state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NarrationEvent {
    Death {
        victim: PlayerId,
        role: Role,
        method: KillMethod,
        /// Whether the group shows roles on death (`ShowRolesDeath`). The
        /// role is always carried; this says whether to say it.
        reveal_role: bool,
        /// The public line for this death (see `death_text`), or `None`
        /// when another line already announced it.
        message: Option<crate::messages::Message>,
    },
    /// A role transform resolved (Werewolf.cs's `Transform(...)` calls) —
    /// Traitor becoming a Wolf, Doppelganger copying a dead role model,
    /// etc. `from`/`to` rather than just `to` because several legacy
    /// messages (`DGToWolf`, `DGToCult`, ...) are phrased around what the
    /// player *was*, not just what they became.
    Transform { player: PlayerId, from: Role, to: Role },
    /// The game has ended. Carries the whole `WinOutcome`, not just a
    /// `Team`: a game that hit the round cap ends with `Unimplemented`,
    /// and an admin's `/killgame` with `Killed`.
    GameOver {
        winner: crate::WinOutcome,
        /// The winner's line (see `game_over_message`).
        message: Option<crate::messages::Message>,
    },
    /// Something told to one player only — see `messages::Note`.
    Private { to: PlayerId, note: Note },
    /// A whole message told to one player only, for what `Note` can't
    /// carry: role info and team lists (see `role_info`).
    Tell {
        to: PlayerId,
        message: crate::messages::Message,
    },
    /// Something told to the whole group — see `messages::Announcement`.
    Announce(Announcement),
    /// The end-of-game summary, right after `GameOver`.
    GameSummary(crate::summary::GameSummary),
}

/// Maps a death to the legacy flavor key, where there is one.
///
/// The legacy `*Killed` keys (`GunnerKilled`, `SeerKilled`, `PrinceKilled`,
/// ...) are all Serial-Killer-specific flavor text, not general per-role
/// death messages (confirmed by reading their actual English.xml values —
/// every one mentions "the serial killer has struck again"), so they're
/// only used for `KillMethod::SerialKilled` specifically, never for a
/// lynch/eat/etc. death of the same role — `DefaultKilled` covers every
/// role without its own dedicated key. `KillMethod::VisitWolf` (a Harlot
/// visiting the wolves' actual target, see the Harlot region of `night`)
/// gets the real public death text too (`HarlotFuckedWolfPublic` — yes,
/// that's the actual legacy key name).
///
/// `KillMethod::LoverDied`/`HunterShot` are still `None`: their real text
/// needs *two* names (the lover who died first and the one dying of
/// grief; the shooter and the target) plus a role reveal, and
/// `NarrationEvent::Death` only carries one victim — giving them real
/// flavor text needs that event to grow a second identity field first,
/// which is more than this batch of "easy, unambiguous" mappings signed
/// up for.
///
/// The lines that depend on the victim's role (eaten, killed by the Serial
/// Killer, fallen into a grave, killed visiting the Serial Killer) are the
/// role's `RoleDef::deaths`. `KillMethod::Eat` covers the wolves' kill
/// and the Wolf Cub's bonus kill alike (Werewolf.cs:4218-4267).
pub fn death_locale_key(role: Role, method: KillMethod) -> Option<&'static str> {
    match method {
        KillMethod::Lynch => Some("LynchKill"),
        // The Harlot dying by visiting the wolves' actual target
        // (Werewolf.cs:2359-2360, `killedByRole: IRole.Wolf`). Matched to
        // her own case in the "died by visiting" switch
        // (Werewolf.cs:4358-4364: `DiedByVisitingKiller` + `KilledByRole
        // == Wolf` => `"HarlotFuckedWolfPublic"`) — not
        // `"HarlotFuckKillerPublic"`, which is the *SerialKiller*-visited
        // variant of the same switch (Werewolf.cs:4368-4369).
        KillMethod::VisitWolf => Some("HarlotFuckedWolfPublic"),
        // Whoever dies visiting the Serial Killer player themself
        // (Werewolf.cs:2339-2347's shared `VisitPlayer` gate, reachable
        // for three different visitors this port now models: a Harlot who
        // picked the SK as her target, a wolf who drew the short straw in
        // the wolves region of `night`, or the cult's
        // newbie in the cult region of `night`). Genuinely three
        // different real messages depending on the dying role — the
        // "died by visiting" switch (Werewolf.cs:4322-4406) branches on
        // `p.PlayerRole` before it ever looks at `KilledByRole`, so a
        // wolf and a Harlot dying to the exact same mechanic still get
        // different text.
        // Werewolf.cs:4322-4406: the line depends on who walked in.
        KillMethod::VisitKiller => def(role).deaths.visited_serial_killer,
        // A Cultist killed by the Cultist Hunter, hunted or while trying
        // to convert them (Werewolf.cs:4342-4346).
        KillMethod::Hunt => Some("HunterKilledCultist"),
        // The Harlot visiting whoever the Serial Killer separately killed
        // that same night — a different player from the SK themself
        // (Werewolf.cs:3893-3897 → `KilledByRole == SerialKiller` +
        // `DiedByVisitingVictim`, 4370-4371).
        KillMethod::VisitVictim => Some("HarlotFuckedKilledPublic"),
        KillMethod::Eat => Some(def(role).deaths.eaten),
        // Werewolf.cs:4251-4262.
        KillMethod::SerialKilled => Some(def(role).deaths.serial_killed),
        // Werewolf.cs:4288-4302, the "killed by visiting Grave Digger"
        // switch.
        KillMethod::FallGrave => Some(def(role).deaths.fell_in_grave),
        // The Chemist region of `night` produces this for either the target (success)
        // or the Chemist themself (failure) - two different real messages
        // (Werewolf.cs:3801-3814: `ChemistSuccessPublic`/`ChemistFailPublic`),
        // each needing only the one name involved (unlike `HunterShot`/
        // `LoverDied`, which need two and stay unmapped for exactly that
        // reason). `role == Chemist` is what tells the two apart here: if
        // the Chemist is the one who died, this had to be the failure
        // case, not a coincidence of some *other* dead Chemist - only one
        // Chemist exists per game, the same uniqueness every other
        // single-instance role in this switch already relies on.
        KillMethod::Chemistry => Some(if role == Role::Chemist {
            "ChemistFailPublic"
        } else {
            "ChemistSuccessPublic"
        }),
        // Werewolf.cs:848: `SendWithQueue(GetLocaleString("Flee", p
        // .GetName()))`, sent unconditionally the moment a mid-game flee
        // resolves - real, role-agnostic, single-name text, unlike most
        // of this switch.
        KillMethod::Flee => Some("Flee"),
        _ => None,
    }
}

/// Maps a game-ending `WinOutcome` to its real legacy per-team victory
/// key. Only defined for an actual `Team` win — the RNG/data-gap
/// `WinOutcome` variants (`RngDependent`, `InsufficientData`,
/// `Unimplemented`) have no legacy equivalent to look up, since the real
/// game always resolved to a concrete winner one way or another; that gap
/// is exactly what those variants exist to flag (see `WinOutcome`'s own
/// doc). `Team::Thief` has no key either: it is
/// the Thief's transient pre-steal team, never itself a winning faction
/// in `evaluate_winner_with_kills` (checked: nothing produces
/// `WinOutcome::Team(Team::Thief)` today). `Team::NoOne` is `NoWinner`
/// (Werewolf.cs:4810). `Thief` is still named explicitly
/// rather than folded behind the RNG/data-gap arm below, so a reader
/// isn't left wondering whether their absence is an oversight.
pub fn game_over_locale_key(outcome: &crate::WinOutcome) -> Option<&'static str> {
    use crate::WinOutcome;
    match outcome {
        WinOutcome::Team(Team::Village) => Some("VillageWins"),
        WinOutcome::Team(Team::Wolf) => Some("WolfWins"),
        WinOutcome::Team(Team::Tanner) => Some("TannerWins"),
        WinOutcome::Team(Team::Cult) => Some("CultWins"),
        WinOutcome::Team(Team::SerialKiller) => Some("SerialKillerWins"),
        WinOutcome::Team(Team::Arsonist) => Some("ArsonistWins"),
        WinOutcome::Team(Team::Lovers) => Some("LoversWin"),
        WinOutcome::Team(Team::NoOne) => Some("NoWinner"),
        WinOutcome::Team(Team::Thief)
        | WinOutcome::RngDependent(_)
        | WinOutcome::InsufficientData(_)
        | WinOutcome::Unimplemented(_)
        | WinOutcome::Killed => None,
    }
}

/// The winner's line: `game_over_locale_key`, except that more than one
/// wolf left makes it `WolvesWin` (Werewolf.cs:4816-4825).
pub fn game_over_message(outcome: &crate::WinOutcome, several_wolves: bool) -> Option<crate::messages::Message> {
    let key = match outcome {
        crate::WinOutcome::Team(Team::Wolf) if several_wolves => "WolvesWin",
        _ => game_over_locale_key(outcome)?,
    };
    Some(crate::messages::Message::new(key, vec![]))
}

/// Every distinct locale key `prompt_locale_key`/`death_locale_key` can
/// currently produce, in one place — the input to the locale-coverage
/// completeness check (`sim/tests/locale_coverage.rs`), which measures
/// "how much of English.xml does this port actually reach" as a real
/// number instead of a vibe. Grows every time a mapping table grows;
/// nothing computes this automatically today (there's no exhaustive
/// `Role`/`KillMethod` iterator to drive `death_locale_key` with yet, the
/// same "two hardcoded levels are enough until a second case shows up"
/// tradeoff as the night's two-batch dependency resolution), so this is
/// hand-kept in sync rather than derived — small enough today that a
/// missed update would be obvious in the coverage number itself.
pub fn all_mapped_locale_keys() -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = ALL_PROMPTS.iter().filter_map(|&p| prompt_locale_key(p)).collect();
    keys.extend([
        // death_locale_key's non-`None` outputs.
        "LynchKill",
        "HarlotFuckedWolfPublic",
        "HarlotFuckKillerPublic",
        "HarlotFuckedKilledPublic",
        "SerialKillerKilledWolf",
        "SnowFrozeKiller",
        "CultConvertKillerPublic",
        "SerialKillerKilledCH",
        "ThiefStoleKiller",
        "HunterKilledCultist",
        // death_text's lines beyond death_locale_key.
        "Was",
        "IsDead",
        "PlayerRoleWas",
        "GenericDeathNoReveal",
        "DefaultShot",
        "HarlotShot",
        "HunterKilledFinalShot",
        "HunterKilledFinalLynched",
        "HunterShotWolf",
        "HunterShotWolfMulti",
        "SnowFrozeHunter",
        "HunterKilledVisiter",
        "LoverDied",
        "ApprenticeSeerEaten",
        "DetectiveEaten",
        "DrunkEaten",
        "FoolEaten",
        "GunnerEaten",
        "HarlotEaten",
        "MasonEaten",
        "SeerEaten",
        "SorcererEaten",
        "WildChildEaten",
        "GuardianEaten",
        "DefaultEaten",
        "GunnerKilled",
        "SeerKilled",
        "DrunkKilled",
        "MayorKilled",
        "PrinceKilled",
        "CultistKilled",
        "GuardianAngelKilled",
        "BlacksmithKilled",
        "CupidKilled",
        "DefaultKilled",
        "HarlotFellPublic",
        "DefaultFellPublic",
        "WolfFellPublic",
        "CultistFellPublic",
        "ThiefFellPublic",
        // death_text's night lines.
        "GAGuardedKiller",
        "ChemistSKPublic",
        "ArsonistVisitKiller",
        "WolfVisitBurn",
        "DefaultVisitBurn",
        "GAGuardedWolf",
        "CultConvertWolfPublic",
        "HarlotFuckedVictimPublic",
        "KillerSpottedDiggerPublic",
        "WolvesSpottedDiggerPublic",
        "ChemistSuccessPublic",
        "ChemistFailPublic",
        "Flee",
        // game_over_locale_key's non-`None` outputs.
        "VillageWins",
        "WolfWins",
        "TannerWins",
        "CultWins",
        "SerialKillerWins",
        "ArsonistWins",
        "LoversWin",
        "NoWinner",
        "WolvesWin",
    ]);
    keys.extend(crate::messages::all_message_keys());
    // Role display names, which notes like `SeerSees` fill in. The four
    // left out have no English.xml entry (see `Role::locale_key`).
    keys.extend(
        shared::ALL_ROLES
            .iter()
            .filter(|r| !matches!(r, Role::Spumpkin | Role::Chef | Role::Barkeep | Role::Witch))
            .map(|r| r.locale_key()),
    );
    keys.sort_unstable();
    keys.dedup();
    keys
}

#[cfg(test)]
mod locale_mapping_tests {
    use super::*;
    use crate::WinOutcome;

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

    #[test]
    fn serial_killed_maps_to_the_dying_roles_own_flavor_key() {
        assert_eq!(
            death_locale_key(Role::Gunner, KillMethod::SerialKilled),
            Some("GunnerKilled")
        );
        assert_eq!(
            death_locale_key(Role::Seer, KillMethod::SerialKilled),
            Some("SeerKilled")
        );
        assert_eq!(
            death_locale_key(Role::Blacksmith, KillMethod::SerialKilled),
            Some("BlacksmithKilled")
        );
        assert_eq!(
            death_locale_key(Role::Cupid, KillMethod::SerialKilled),
            Some("CupidKilled")
        );
    }

    #[test]
    fn serial_killed_falls_back_to_default_for_an_uncelebrated_role() {
        assert_eq!(
            death_locale_key(Role::Villager, KillMethod::SerialKilled),
            Some("DefaultKilled")
        );
    }

    #[test]
    fn a_harlot_dying_from_visiting_a_wolf_has_a_key() {
        assert_eq!(
            death_locale_key(Role::Harlot, KillMethod::VisitWolf),
            Some("HarlotFuckedWolfPublic")
        );
    }

    #[test]
    fn wolf_eat_maps_to_the_dying_roles_own_flavor_key() {
        assert_eq!(death_locale_key(Role::Seer, KillMethod::Eat), Some("SeerEaten"));
        assert_eq!(
            death_locale_key(Role::Detective, KillMethod::Eat),
            Some("DetectiveEaten")
        );
        assert_eq!(
            death_locale_key(Role::GuardianAngel, KillMethod::Eat),
            Some("GuardianEaten")
        );
    }

    #[test]
    fn wolf_eat_falls_back_to_default_for_an_uncelebrated_role() {
        assert_eq!(death_locale_key(Role::Villager, KillMethod::Eat), Some("DefaultEaten"));
    }

    #[test]
    fn chemistry_success_and_failure_map_to_different_keys() {
        assert_eq!(
            death_locale_key(Role::Villager, KillMethod::Chemistry),
            Some("ChemistSuccessPublic")
        );
        assert_eq!(
            death_locale_key(Role::Chemist, KillMethod::Chemistry),
            Some("ChemistFailPublic")
        );
    }

    #[test]
    fn visit_killer_deaths_vary_by_the_dying_roles_own_team() {
        assert_eq!(
            death_locale_key(Role::Wolf, KillMethod::VisitKiller),
            Some("SerialKillerKilledWolf")
        );
        assert_eq!(
            death_locale_key(Role::AlphaWolf, KillMethod::VisitKiller),
            Some("SerialKillerKilledWolf")
        );
        assert_eq!(
            death_locale_key(Role::SnowWolf, KillMethod::VisitKiller),
            Some("SnowFrozeKiller")
        );
        assert_eq!(
            death_locale_key(Role::Cultist, KillMethod::VisitKiller),
            Some("CultConvertKillerPublic")
        );
        assert_eq!(
            death_locale_key(Role::Harlot, KillMethod::VisitKiller),
            Some("HarlotFuckKillerPublic")
        );
        assert_eq!(death_locale_key(Role::Villager, KillMethod::VisitKiller), None);
    }

    #[test]
    fn flee_has_one_role_agnostic_key() {
        assert_eq!(death_locale_key(Role::Villager, KillMethod::Flee), Some("Flee"));
        assert_eq!(death_locale_key(Role::Seer, KillMethod::Flee), Some("Flee"));
    }

    #[test]
    fn lover_died_and_hunter_shot_still_have_no_key() {
        assert_eq!(death_locale_key(Role::Villager, KillMethod::LoverDied), None);
        assert_eq!(death_locale_key(Role::Villager, KillMethod::HunterShot), None);
    }

    #[test]
    fn known_win_outcomes_map_to_their_real_keys() {
        assert_eq!(
            game_over_locale_key(&WinOutcome::Team(Team::Village)),
            Some("VillageWins")
        );
        assert_eq!(game_over_locale_key(&WinOutcome::Team(Team::Wolf)), Some("WolfWins"));
        assert_eq!(game_over_locale_key(&WinOutcome::Team(Team::Lovers)), Some("LoversWin"));
    }

    #[test]
    fn rng_dependent_outcomes_have_no_key_and_no_one_is_no_winner() {
        assert_eq!(game_over_locale_key(&WinOutcome::Team(Team::NoOne)), Some("NoWinner"));
        assert_eq!(game_over_locale_key(&WinOutcome::RngDependent("test")), None);
    }
}

/// The seam: real I/O (or a test double) lives entirely behind this trait.
/// Async because a real Telegram presenter is fundamentally waiting on
/// network events (a callback query arriving), not something that can be
/// answered synchronously. `?Send`: everything in this crate runs on a
/// single-threaded executor (current-thread `tokio` in tests, and a real
/// bot has no need to hand a `dyn Presenter` across threads), so requiring
/// `Send` futures — `async_trait`'s default — is a constraint nothing
/// here actually needs, and it broke the moment `ask_toggle` got a
/// default body.
#[async_trait(?Send)]
pub trait Presenter {
    /// Ask `player` to pick `count` distinct players from `options` for
    /// the given `prompt`. Returns `None` if the player declines/times out
    /// (a legitimate answer, e.g. Harlot choosing to stay home) — an
    /// implementer that returns `Some(v)` should return exactly `count`
    /// entries; the orchestrator treats anything else as "no answer"
    /// rather than trusting a malformed response. `Some` of an empty list
    /// means the player pressed Skip (see `Answer::Targets`).
    async fn ask_targets(
        &mut self,
        player: PlayerId,
        prompt: Prompt,
        options: &[PlayerId],
        count: usize,
    ) -> Option<Vec<PlayerId>>;

    /// A genuinely different question shape from `ask_targets`: yes/no,
    /// no player selection at all (Sandman's "sleep everyone?", Mayor's
    /// "reveal?", Pacifist's "veto the lynch?"). Kept as its own method
    /// rather than shoehorned into `ask_targets` with `count: 0` — that
    /// would be reusing one method for two different question shapes,
    /// the mirror-image mistake of collapsing `ask_target`/
    /// `ask_two_targets` into one arity-parameterized method. Default
    /// `false`: most presenters (and most roles) never need this.
    async fn ask_toggle(&mut self, _player: PlayerId, _prompt: Prompt) -> bool {
        false
    }

    /// Called by `run_game` between rounds (after both night and day have
    /// resolved with no winner yet). Default no-op — a real Telegram
    /// presenter naturally knows what round it's in from its own state,
    /// it doesn't need telling. `sim::FixturePresenter` is the one
    /// implementer that overrides this, to advance which historical day
    /// it answers questions from.
    fn advance_round(&mut self) {}

    /// Tells the presenter something happened, worth narrating, that
    /// isn't a question — see `NarrationEvent`. Called by `run_game`
    /// *at the moment* each event happens (right after a death is
    /// resolved, before moving on), not reconstructed afterward from
    /// `GameOutcome` — that's what keeps a transcript's death
    /// announcements interleaved with the questions that led to them,
    /// in the actual order they happened, instead of one batch at the
    /// end. Default no-op: most presenters (and every existing one
    /// before this hook existed) have no use for it, same reasoning as
    /// `ask_toggle`'s default.
    async fn narrate(&mut self, _event: NarrationEvent) {}

    /// Players who asked to flee since the last time this was checked
    /// (Werewolf.cs's `RemovePlayer`, the `IsRunning` branch,
    /// 848-860) — a mid-game leave, not the joining-phase one
    /// `control::lobby::Lobby::leave` already covers.
    ///
    /// The legacy game reacts to a flee the instant the command arrives,
    /// mid-question if need be. The engine cannot interrupt an `await`,
    /// so `game::run_game_with_rules` polls this wherever it checks for
    /// the end of the game (after each step of the round). A flee lands
    /// at the next checkpoint, not instantly. A Telegram-backed presenter
    /// would drain whatever `/flee` commands arrived since the last
    /// poll. Default: nobody ever flees, so every existing test/fixture
    /// presenter's behavior is unchanged without needing to implement
    /// this.
    fn drain_fled_players(&mut self) -> Vec<PlayerId> {
        vec![]
    }

    /// Has an admin ended this game outright since the last check
    /// (`/killgame`, Werewolf.cs `Kill()`, 5393-5406)? Polled at the same
    /// checkpoints as `drain_fled_players`, for the same reason (no
    /// interrupt-mid-`await` mechanism) — see `game::run_game`, which
    /// treats a `true` here as an immediate `WinOutcome::Killed`, no
    /// further night/day resolution. Default `false`: every existing
    /// test/fixture presenter's behavior is unchanged.
    fn game_was_killed(&mut self) -> bool {
        false
    }

    /// Asks a whole phase's questions at once and returns one answer per
    /// question, in order. The legacy game sends every night prompt
    /// together under one timer (`SendNightActions`) and every lynch vote
    /// together under another, so a real presenter overrides this to send
    /// them all and wait once; asking one player after another would make
    /// a ten-player night ten timers long.
    ///
    /// `on_answer` is the engine's reaction to each answer the moment it
    /// lands, before the rest of the batch is in: a presenter calls it as
    /// each answer arrives and narrates what it returns right away. That
    /// is how a lynch vote is announced when it is cast
    /// (Werewolf.cs:1125-1137), not after everyone has voted.
    ///
    /// The default asks one question at a time through `ask_targets` and
    /// `ask_toggle`, in order, calling `on_answer` after each, so a
    /// presenter that doesn't override it sees exactly the calls it always
    /// did.
    async fn ask_batch(
        &mut self,
        _phase: Phase,
        questions: Vec<Question>,
        on_answer: &mut dyn OnAnswer,
    ) -> Vec<Answer> {
        let mut answers = Vec::with_capacity(questions.len());
        for (index, question) in questions.into_iter().enumerate() {
            if on_answer.closed(index) {
                answers.push(match question {
                    Question::Targets { .. } => Answer::Targets(None),
                    Question::Toggle { .. } => Answer::Toggle(false),
                });
                continue;
            }
            let answer = match question {
                Question::Targets {
                    player,
                    prompt,
                    options,
                    count,
                    ..
                } => Answer::Targets(self.ask_targets(player, prompt, &options, count).await),
                Question::Toggle { player, prompt } => Answer::Toggle(self.ask_toggle(player, prompt).await),
            };
            for event in on_answer.answered(index, &answer) {
                self.narrate(event).await;
            }
            answers.push(answer);
        }
        answers
    }
}

/// Which phase a batch of questions belongs to, so a presenter can pick
/// the right timer: the night's, or the day's (role actions for the whole
/// day, then the lynch vote).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Night,
    Day,
}

/// One question in a batch: the same two shapes as `ask_targets` and
/// `ask_toggle`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Question {
    Targets {
        player: PlayerId,
        prompt: Prompt,
        options: Vec<PlayerId>,
        count: usize,
        /// What to ask, when it's more than the prompt's own key (the
        /// Arsonist's question lists the houses already doused).
        text: Option<crate::messages::Message>,
    },
    Toggle {
        player: PlayerId,
        prompt: Prompt,
    },
}

impl Question {
    pub fn player(&self) -> PlayerId {
        match self {
            Question::Targets { player, .. } | Question::Toggle { player, .. } => *player,
        }
    }

    pub fn prompt(&self) -> Prompt {
        match self {
            Question::Targets { prompt, .. } | Question::Toggle { prompt, .. } => *prompt,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// `None`: timed out or not answered. `Some` of an empty list: the
    /// player pressed Skip, which only the Hunter's last shot tells apart
    /// from no answer; everywhere else it counts as no answer.
    Targets(Option<Vec<PlayerId>>),
    Toggle(bool),
}

/// See `Presenter::ask_batch`.
pub trait OnAnswer {
    /// Answer `index` of the batch just landed; returns what to tell the
    /// group or the player now. Called at most once per question.
    fn answered(&mut self, index: usize, answer: &Answer) -> Vec<NarrationEvent>;

    /// Whether question `index` is no longer wanted: a presenter stops
    /// waiting for it, and once every question it still waits on is
    /// closed, the batch is over. A lynch vote is closed by a Pacifist's
    /// peace (Werewolf.cs:2575-2590, which also edits the open menus to
    /// say so: `LynchPeaceTimeout`). Default: nothing closes early.
    fn closed(&self, _index: usize) -> bool {
        false
    }

    /// The key a presenter edits a closed question's message to.
    fn closed_key(&self, _index: usize) -> &'static str {
        "TimesUp"
    }
}

/// For batches nothing needs to react to as they're answered.
pub struct Quiet;

impl OnAnswer for Quiet {
    fn answered(&mut self, _: usize, _: &Answer) -> Vec<NarrationEvent> {
        vec![]
    }
}

/// Tells a wolf's or a Cultist's teammates whom they voted for, as each
/// vote lands (Werewolf.cs:1030-1046 `WolfVotedKill`, 1112-1122
/// `CultistVotedConvert`).
pub struct TeamVotes {
    /// Per question: the voter, who hears their vote, and the key.
    asked: HashMap<usize, (PlayerId, Vec<PlayerId>, &'static str)>,
    options: Vec<Vec<PlayerId>>,
}

impl TeamVotes {
    pub fn for_night(questions: &[Question], players: &[AlivePlayer]) -> Self {
        let team = |voter: PlayerId, of: fn(Role) -> bool| -> Vec<PlayerId> {
            players
                .iter()
                .filter(|p| p.id != voter && of(p.role))
                .map(|p| p.id)
                .collect()
        };
        let mut asked = HashMap::new();
        let mut options = vec![];
        for (index, q) in questions.iter().enumerate() {
            let Question::Targets {
                player,
                prompt,
                options: offered,
                ..
            } = q
            else {
                options.push(vec![]);
                continue;
            };
            options.push(offered.clone());
            let heard = match prompt {
                Prompt::WolfEat | Prompt::WolfCubBonusKill => {
                    Some((team(*player, Role::is_eating_wolf), "WolfVotedKill"))
                }
                Prompt::CultistConvert => Some((team(*player, |r| r == Role::Cultist), "CultistVotedConvert")),
                _ => None,
            };
            if let Some((to, key)) = heard {
                asked.insert(index, (*player, to, key));
            }
        }
        TeamVotes { asked, options }
    }
}

impl OnAnswer for TeamVotes {
    fn answered(&mut self, index: usize, answer: &Answer) -> Vec<NarrationEvent> {
        let (Some((voter, to, key)), Answer::Targets(Some(picked))) = (self.asked.get(&index), answer) else {
            return vec![];
        };
        let [target] = picked.as_slice() else { return vec![] };
        if !self.options[index].contains(target) {
            return vec![];
        }
        let message = Message::new(key, vec![MessageArg::Player(*voter), MessageArg::Player(*target)]);
        to.iter()
            .map(|&to| NarrationEvent::Tell {
                to,
                message: message.clone(),
            })
            .collect()
    }
}

/// Sends one phase's questions and returns validated answers, one per
/// question (see `validate_pick`); an answer of the wrong shape, or a
/// missing one, counts as no answer.
async fn ask_phase(
    presenter: &mut dyn Presenter,
    phase: Phase,
    questions: &[Question],
    on_answer: &mut dyn OnAnswer,
) -> Vec<Answer> {
    if questions.is_empty() {
        return vec![];
    }
    let mut answers = presenter
        .ask_batch(phase, questions.to_vec(), on_answer)
        .await
        .into_iter();
    questions.iter().map(|q| validate_answer(q, answers.next())).collect()
}

fn validate_answer(question: &Question, answer: Option<Answer>) -> Answer {
    match (question, answer) {
        (Question::Targets { options, count, .. }, Some(Answer::Targets(Some(picked)))) => {
            Answer::Targets(validate_pick(picked, options, *count))
        }
        (Question::Targets { .. }, _) => Answer::Targets(None),
        (Question::Toggle { .. }, Some(Answer::Toggle(b))) => Answer::Toggle(b),
        (Question::Toggle { .. }, _) => Answer::Toggle(false),
    }
}

/// What counts as a well-formed pick: exactly `count` entries, all
/// distinct, all among the options offered. An answer outside the options
/// (a stale button for a player who has since died, say) counts as no
/// answer, never as a pick.
fn validate_pick(picked: Vec<PlayerId>, options: &[PlayerId], count: usize) -> Option<Vec<PlayerId>> {
    let all_distinct = (1..picked.len()).all(|i| !picked[..i].contains(&picked[i]));
    let all_offered = picked.iter().all(|p| options.contains(p));
    (picked.len() == count && all_distinct && all_offered).then_some(picked)
}

/// Builds a batch while remembering where each question landed.
#[derive(Default)]
struct BatchBuilder {
    questions: Vec<Question>,
}

impl BatchBuilder {
    fn targets(&mut self, player: PlayerId, prompt: Prompt, options: &[PlayerId], count: usize) -> usize {
        self.questions.push(Question::Targets {
            player,
            prompt,
            options: options.to_vec(),
            count,
            text: None,
        });
        self.questions.len() - 1
    }

    fn targets_with_text(
        &mut self,
        player: PlayerId,
        prompt: Prompt,
        options: &[PlayerId],
        text: crate::messages::Message,
    ) -> usize {
        self.questions.push(Question::Targets {
            player,
            prompt,
            options: options.to_vec(),
            count: 1,
            text: Some(text),
        });
        self.questions.len() - 1
    }

    fn toggle(&mut self, player: PlayerId, prompt: Prompt) -> usize {
        self.questions.push(Question::Toggle { player, prompt });
        self.questions.len() - 1
    }
}

fn targets_answer(answers: &[Answer], index: Option<usize>) -> Option<Vec<PlayerId>> {
    match index.map(|i| &answers[i]) {
        Some(Answer::Targets(t)) => t.clone(),
        _ => None,
    }
}

fn single_target_answer(answers: &[Answer], index: Option<usize>) -> Option<PlayerId> {
    targets_answer(answers, index).and_then(|v| v.into_iter().next())
}

fn toggle_answer(answers: &[Answer], index: Option<usize>) -> bool {
    matches!(index.map(|i| &answers[i]), Some(Answer::Toggle(true)))
}

/// What every alive player brings into either phase: who they are and
/// what role they're currently playing. Shared by `resolve_night_with_freeze`
/// and `resolve_day` — a player's identity and role don't change between the
/// two, only which questions get asked.
#[derive(Debug, Clone, Copy)]
pub struct AlivePlayer {
    pub id: PlayerId,
    pub role: Role,
}

/// Who a night menu offers (Werewolf.cs:5157-5212): every other living
/// player, less the wolves and the Snow Wolf for a wolf, and less the
/// cult for a Cultist.
fn night_options(player: &AlivePlayer, players: &[AlivePlayer]) -> Vec<PlayerId> {
    let eats = Role::is_eating_wolf;
    players
        .iter()
        .filter(|o| o.id != player.id)
        .filter(|o| !eats(player.role) || !(eats(o.role) || o.role == Role::SnowWolf))
        .filter(|o| player.role != Role::Cultist || o.role != Role::Cultist)
        .map(|o| o.id)
        .collect()
}

/// Asks every night question and turns the answers into `NightAction`s,
/// each paired with the player who took it (so the Snow Wolf's freeze,
/// `freeze::apply_freeze`, can cancel one player's actions). The Snow
/// Wolf's freeze question (`freeze::freeze_question`) goes out in the same
/// batch as everyone else's. Returns the actions, the wolves' target (the
/// legacy count, `night::wolf_vote_winner`), and the freeze picked as
/// `(snow_wolf, target)`. What the actions do is `night::resolve_night_sequence`.
///
/// Roles asked after the wolves (`NightQuestion::AfterWolves`: the Witch,
/// who needs the wolves' target) get a second batch once it is known.
///
/// Players in `skip` get no menu tonight: drunk wolves, and wolves after
/// the Blacksmith's silver (Werewolf.cs:5191, 5316). A Chemist who hasn't
/// brewed brews instead of being asked (Werewolf.cs:5259-5271). The
/// Arsonist is offered only houses not yet `doused`, is shown those that
/// are, and can spark only once there are some (Werewolf.cs:5280-5292).
pub async fn resolve_night_with_freeze(
    players: &[AlivePlayer],
    states: &mut HashMap<PlayerId, RoleState>,
    freeze: Option<(PlayerId, Vec<PlayerId>)>,
    presenter: &mut dyn Presenter,
    skip: &[PlayerId],
    doused: &[PlayerId],
) -> (
    Vec<(PlayerId, NightAction)>,
    Option<PlayerId>,
    Option<(PlayerId, PlayerId)>,
) {
    let alive: Vec<PlayerId> = players.iter().map(|p| p.id).collect();
    let mut actions: Vec<(PlayerId, NightAction)> = vec![];

    // --- Stage 1: roles with no dependencies, all asked at once ---
    struct Asked {
        player: AlivePlayer,
        cupid: Option<usize>,
        target: Option<usize>,
        toggle: Option<usize>,
    }
    let mut batch = BatchBuilder::default();
    let freeze_index = freeze
        .as_ref()
        .map(|(snow_wolf, options)| batch.targets(*snow_wolf, Prompt::SnowWolfFreeze, options, 1));
    let mut asked = vec![];
    for player in players {
        let night = def(player.role).night;
        if night == NightQuestion::AfterWolves || skip.contains(&player.id) {
            continue; // asked in stage 2, or not asked
        }
        let state = states.entry(player.id).or_default();
        if matches!(night, NightQuestion::BrewThenPick(_)) && !state.primary_used {
            state.primary_used = true;
            let note = crate::messages::Note::Night(crate::messages::NightNote::ChemistBrewing);
            presenter.narrate(NarrationEvent::Private { to: player.id, note }).await;
            continue;
        }
        let mut a = Asked {
            player: *player,
            cupid: None,
            target: None,
            toggle: None,
        };
        let options = || night_options(player, players);
        match night {
            NightQuestion::Pair(prompt) => a.cupid = Some(batch.targets(player.id, prompt, &alive, 2)),
            NightQuestion::Pick(prompt) | NightQuestion::BrewThenPick(prompt) => {
                a.target = Some(batch.targets(player.id, prompt, &options(), 1));
            }
            // A role model is picked once (Werewolf.cs:5222-5237).
            NightQuestion::RoleModel(prompt) if !state.primary_used => {
                a.target = Some(batch.targets(player.id, prompt, &options(), 1));
            }
            NightQuestion::DouseOrSpark => {
                let options: Vec<PlayerId> = alive
                    .iter()
                    .copied()
                    .filter(|&id| id != player.id && !doused.contains(&id))
                    .collect();
                let living_doused: Vec<PlayerId> = alive.iter().copied().filter(|id| doused.contains(id)).collect();
                let text = if living_doused.is_empty() {
                    Message::new("AskArsonistNoDoused", vec![])
                } else {
                    Message::new("AskArsonist", vec![]).followed_by(Message::new(
                        "AlreadyDousedList",
                        vec![MessageArg::PlayersAnd(living_doused.clone())],
                    ))
                };
                a.target = Some(batch.targets_with_text(player.id, Prompt::ArsonistDouse, &options, text));
                if !living_doused.is_empty() {
                    a.toggle = Some(batch.toggle(player.id, Prompt::ArsonistSpark));
                }
            }
            NightQuestion::RoleModel(_) | NightQuestion::None | NightQuestion::Freeze | NightQuestion::AfterWolves => {}
        }
        asked.push(a);
    }
    let mut votes = TeamVotes::for_night(&batch.questions, players);
    let answers = ask_phase(presenter, Phase::Night, &batch.questions, &mut votes).await;

    let freeze = freeze
        .zip(single_target_answer(&answers, freeze_index))
        .map(|((snow_wolf, _), target)| (snow_wolf, target));
    for a in &asked {
        let behavior = def(a.player.role).behavior;
        let state = states.entry(a.player.id).or_default();
        let ctx = NightContext {
            alive: &alive,
            self_id: a.player.id,
            chosen_target: single_target_answer(&answers, a.target),
            heal_target: None,
            poison_target: None,
            love_targets: targets_answer(&answers, a.cupid).map(|v| (v[0], v[1])),
            wolf_target: None,
            toggle_choice: toggle_answer(&answers, a.toggle),
        };
        actions.extend(
            behavior
                .night_action(&ctx, state)
                .into_iter()
                .map(|act| (a.player.id, act)),
        );
    }

    let wolf_target = crate::night::wolf_vote_winner(
        players.iter().map(|p| p.id),
        actions.iter().filter_map(|(_, a)| match a {
            NightAction::EatVote { target } => Some(*target),
            _ => None,
        }),
    );

    // --- Stage 2: roles depending on a fact produced above, asked
    // together once that fact is known ---
    let mut batch = BatchBuilder::default();
    let mut asked = vec![];
    for player in players {
        if def(player.role).night != NightQuestion::AfterWolves {
            continue; // already handled in stage 1
        }
        // The Witch is the only stage-2 role: both her potion questions.
        let heal = batch.targets(player.id, Prompt::WitchHeal, &alive, 1);
        let poison = batch.targets(player.id, Prompt::WitchPoison, &alive, 1);
        asked.push((*player, heal, poison));
    }
    let answers = ask_phase(presenter, Phase::Night, &batch.questions, &mut Quiet).await;
    for (player, heal, poison) in asked {
        let behavior = def(player.role).behavior;
        let state = states.entry(player.id).or_default();
        let ctx = NightContext {
            alive: &alive,
            self_id: player.id,
            chosen_target: None,
            heal_target: single_target_answer(&answers, Some(heal)),
            poison_target: single_target_answer(&answers, Some(poison)),
            love_targets: None,
            wolf_target,
            toggle_choice: false,
        };
        actions.extend(behavior.night_action(&ctx, state).into_iter().map(|a| (player.id, a)));
    }

    (actions, wolf_target, freeze)
}

/// The lynch count: most votes wins, and a tie lynches nobody
/// (Werewolf.cs:2690-2711). The wolves count differently, see
/// `night::wolf_vote_winner`.
fn lynch_majority(votes: impl Iterator<Item = PlayerId>) -> Option<PlayerId> {
    let mut counts: HashMap<PlayerId, usize> = HashMap::new();
    for v in votes {
        *counts.entry(v).or_insert(0) += 1;
    }
    let max_count = *counts.values().max()?;
    let mut leaders = counts.iter().filter(|&(_, &c)| c == max_count);
    let first = leaders.next()?;
    if leaders.next().is_some() {
        None // tie
    } else {
        Some(*first.0)
    }
}

/// How a lynch vote came out (Werewolf.cs:2690-2711). The legacy game
/// never re-votes on a tie: `Settings.RandomLynch` is hard-coded `false`,
/// so a tie simply means nobody is lynched today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LynchResult {
    Lynched(PlayerId),
    Tie,
    NoVotes,
}

impl LynchResult {
    pub fn target(self) -> Option<PlayerId> {
        match self {
            LynchResult::Lynched(target) => Some(target),
            LynchResult::Tie | LynchResult::NoVotes => None,
        }
    }

    /// The group message for a vote that lynched nobody.
    pub fn announcement(self) -> Option<Announcement> {
        match self {
            LynchResult::Lynched(_) => None,
            LynchResult::Tie => Some(Announcement::LynchTie),
            LynchResult::NoVotes => Some(Announcement::NoLynchVotes),
        }
    }
}

/// Collects lynch votes as they are cast, announcing each one
/// (Werewolf.cs:1125-1137). Nobody can vote for themselves
/// (Werewolf.cs:4959: `x.Id != player.Id`). A Clumsy Guy's vote has a 50%
/// chance of landing on a random living player other than themselves
/// instead (Werewolf.cs:999-1011, `ChooseRandomPlayerId(player, false)`),
/// which may happen to be the player they picked; the announcement names
/// where it landed.
struct LynchVotes<'a> {
    /// Batch index of each voter's question, with the voter and the
    /// players they could vote for.
    ballots: Vec<(usize, AlivePlayer, Vec<PlayerId>)>,
    alive: usize,
    rules: &'a GameRules,
    rng: &'a mut dyn RngSource,
    /// `(voter, target)`, in the order the votes landed.
    cast: Vec<(PlayerId, PlayerId)>,
    /// Players shot today, who can't vote or be voted for.
    excluded: Vec<PlayerId>,
}

impl<'a> LynchVotes<'a> {
    fn new(rules: &'a GameRules, rng: &'a mut dyn RngSource, alive: usize) -> Self {
        LynchVotes {
            ballots: vec![],
            alive,
            rules,
            rng,
            cast: vec![],
            excluded: vec![],
        }
    }

    fn ask(&mut self, batch: &mut BatchBuilder, voter: &AlivePlayer, alive: &[PlayerId]) {
        let options: Vec<PlayerId> = alive.iter().copied().filter(|&id| id != voter.id).collect();
        let index = batch.targets(voter.id, Prompt::LynchVote, &options, 1);
        self.ballots.push((index, *voter, options));
    }
}

impl OnAnswer for LynchVotes<'_> {
    fn answered(&mut self, index: usize, answer: &Answer) -> Vec<NarrationEvent> {
        let Some(at) = self.ballots.iter().position(|(i, _, _)| *i == index) else {
            return vec![];
        };
        let (_, voter, options) = self.ballots.remove(at); // one vote per ballot
        let Answer::Targets(Some(picked)) = answer else {
            return vec![];
        };
        let options: Vec<PlayerId> = options.into_iter().filter(|o| !self.excluded.contains(o)).collect();
        let Some(choice) = validate_pick(picked.clone(), &options, 1).map(|v| v[0]) else {
            return vec![];
        };
        let target = if voter.role == Role::ClumsyGuy && !options.is_empty() && self.rng.chance(50) {
            options[self.rng.pick_index(options.len())]
        } else {
            choice
        };
        self.cast.push((voter.id, target));
        let announcement = match self.rules.secret_lynch {
            None => Announcement::PlayerVotedLynch {
                voter: voter.id,
                target,
            },
            Some(_) => Announcement::PlayerVoteCounts {
                voted: self.cast.len() as u32,
                alive: self.alive as u32,
            },
        };
        vec![NarrationEvent::Announce(announcement)]
    }
}

/// The day's batch: the lynch votes, plus the yes/no abilities, each
/// announced the moment it's used, as the legacy callbacks do
/// (Werewolf.cs:899-973), so a Mayor's reveal lands before the votes it
/// doubles and a Pacifist's peace stops the vote it cancels.
struct DayAnswers<'a> {
    votes: LynchVotes<'a>,
    /// Batch index of each yes/no ability, with who holds it. Only asked
    /// while unused, so a yes always takes effect.
    abilities: Vec<(usize, AlivePlayer)>,
    /// Whether a Pacifist's peace stands: peace sets it and a later
    /// Troublemaker clears it (Werewolf.cs:916-917, 970-971).
    peace: bool,
    /// Batch index of each Gunner's shot, with the Gunner.
    shots: Vec<(usize, AlivePlayer)>,
    /// Gunners whose shot was told, and so stands even if they're shot
    /// after it.
    fired: Vec<PlayerId>,
    /// Who each question in the batch is for, by index.
    owners: Vec<PlayerId>,
    players: &'a [AlivePlayer],
    /// `ShowRolesDeath`, for the shot's line.
    reveal: bool,
}

impl DayAnswers<'_> {
    /// The Gunner's shot kills at once (Werewolf.cs:2873-2898): its line
    /// is told now, and the victim is out of the rest of the day.
    fn shoot(&mut self, gunner: AlivePlayer, answer: &Answer) -> Vec<NarrationEvent> {
        let Answer::Targets(Some(picked)) = answer else {
            return vec![];
        };
        let Some(&target) = picked.first() else { return vec![] };
        let Some(victim) = self.players.iter().find(|p| p.id == target && p.id != gunner.id) else {
            return vec![];
        };
        if self.votes.excluded.contains(&target) {
            return vec![];
        }
        self.votes.excluded.push(target);
        self.fired.push(gunner.id);
        self.votes
            .cast
            .retain(|&(voter, voted)| voter != target && voted != target);
        let message = death_message(&DeathContext {
            victim: target,
            role: victim.role,
            method: KillMethod::Shoot,
            by: Some((gunner.id, Role::Gunner)),
            killer_was_lynched: false,
            reveal: self.reveal,
            pack_size: 0,
            killed_by: Some(Role::Gunner),
        });
        let mut said = vec![NarrationEvent::Death {
            victim: target,
            role: victim.role,
            method: KillMethod::Shoot,
            reveal_role: self.reveal,
            message,
        }];
        if victim.role == Role::WiseElder {
            said.push(NarrationEvent::Announce(Announcement::GunnerShotWiseElder {
                gunner: gunner.id,
                elder: target,
            }));
        }
        said
    }
}

impl OnAnswer for DayAnswers<'_> {
    fn answered(&mut self, index: usize, answer: &Answer) -> Vec<NarrationEvent> {
        if self.closed(index) {
            return vec![];
        }
        if let Some(at) = self.shots.iter().position(|(i, _)| *i == index) {
            let (_, gunner) = self.shots.remove(at);
            return self.shoot(gunner, answer);
        }
        let Some(at) = self.abilities.iter().position(|(i, _)| *i == index) else {
            return self.votes.answered(index, answer);
        };
        let (_, player) = self.abilities.remove(at);
        if *answer != Answer::Toggle(true) {
            return vec![];
        }
        let DayQuestion::Ability { announce, .. } = def(player.role).day else {
            return vec![];
        };
        match player.role {
            Role::Pacifist => self.peace = true,
            Role::Troublemaker => self.peace = false,
            _ => {}
        }
        vec![NarrationEvent::Announce(announce(player.id))]
    }

    fn closed(&self, index: usize) -> bool {
        self.shot_asked(index) || self.peace_closed(index)
    }

    fn closed_key(&self, index: usize) -> &'static str {
        if self.peace_closed(index) && !self.shot_asked(index) {
            "LynchPeaceTimeout"
        } else {
            "TimesUp"
        }
    }
}

impl DayAnswers<'_> {
    /// Question `index` is for someone shot today.
    fn shot_asked(&self, index: usize) -> bool {
        self.owners.get(index).is_some_and(|p| self.votes.excluded.contains(p))
    }

    /// Question `index` is a lynch vote a Pacifist's peace closed.
    fn peace_closed(&self, index: usize) -> bool {
        self.peace && self.votes.ballots.iter().any(|(i, _, _)| *i == index)
    }
}

/// Secret lynch results, most votes first (Werewolf.cs:2717-2741), when
/// the group shows votes. A revealed Mayor appears with a count of 2.
fn secret_lynch_results(
    votes_by_voter: &[(PlayerId, PlayerId)],
    players: &[AlivePlayer],
    states: &HashMap<PlayerId, RoleState>,
    rules: &GameRules,
) -> Option<Announcement> {
    let secret = rules.secret_lynch.filter(|s| s.show_votes)?;
    let mut tallies: Vec<SecretTally> = vec![];
    for &(voter, target) in votes_by_voter {
        let weight = if is_revealed_mayor(voter, players, states) {
            2
        } else {
            1
        };
        let tally = match tallies.iter_mut().find(|t| t.target == target) {
            Some(t) => t,
            None => {
                tallies.push(SecretTally {
                    target,
                    votes: 0,
                    voters: secret.show_voters.then(Vec::new),
                });
                tallies.last_mut().expect("just pushed")
            }
        };
        tally.votes += weight;
        if let Some(voters) = &mut tally.voters {
            voters.push((voter, weight));
        }
    }
    tallies.sort_by_key(|t| std::cmp::Reverse(t.votes));
    Some(Announcement::SecretLynchResults(tallies))
}

fn is_revealed_mayor(voter: PlayerId, players: &[AlivePlayer], states: &HashMap<PlayerId, RoleState>) -> bool {
    players.iter().any(|p| p.id == voter && p.role == Role::Mayor) && states.get(&voter).is_some_and(|s| s.primary_used)
}

/// Counts votes, with a revealed Mayor's vote counting twice on every
/// lynch after the reveal (Werewolf.cs:2651: `p.HasUsedAbility`, which
/// stays set for the rest of the game).
fn tally_lynch(
    votes_by_voter: &[(PlayerId, PlayerId)],
    players: &[AlivePlayer],
    states: &HashMap<PlayerId, RoleState>,
) -> LynchResult {
    if votes_by_voter.is_empty() {
        return LynchResult::NoVotes;
    }
    let votes = votes_by_voter.iter().flat_map(|&(voter, target)| {
        let weight = if is_revealed_mayor(voter, players, states) {
            2
        } else {
            1
        };
        std::iter::repeat_n(target, weight)
    });
    match lynch_majority(votes) {
        Some(target) => LynchResult::Lynched(target),
        None => LynchResult::Tie,
    }
}

/// A lynch vote on its own, with no other day actions — the second vote a
/// Troublemaker forces (Werewolf.cs:2548-2846, the `do ... while
/// (lynchAttempt < 2)` loop re-runs only the lynch).
pub async fn resolve_lynch_vote(
    players: &[AlivePlayer],
    states: &HashMap<PlayerId, RoleState>,
    rules: &GameRules,
    presenter: &mut dyn Presenter,
    rng: &mut dyn RngSource,
) -> LynchResult {
    let alive: Vec<PlayerId> = players.iter().map(|p| p.id).collect();
    let mut batch = BatchBuilder::default();
    let mut votes = LynchVotes::new(rules, rng, alive.len());
    for player in players {
        votes.ask(&mut batch, player, &alive);
    }
    ask_phase(presenter, Phase::Day, &batch.questions, &mut votes).await;
    let votes_by_voter = votes.cast;
    if let Some(results) = secret_lynch_results(&votes_by_voter, players, states, rules) {
        presenter.narrate(NarrationEvent::Announce(results)).await;
    }
    tally_lynch(&votes_by_voter, players, states)
}

/// Resolves one full day: every alive player casts a lynch vote (universal
/// — unlike night actions, this isn't gated by role at all), plus whatever
/// role-specific day actions apply. Like `resolve_night_with_freeze`, this
/// only resolves *decisions* — see `apply_day_results` for turning them into
/// deaths.
pub async fn resolve_day(
    players: &[AlivePlayer],
    states: &mut HashMap<PlayerId, RoleState>,
    rules: &GameRules,
    presenter: &mut dyn Presenter,
    rng: &mut dyn RngSource,
) -> (Vec<DayAction>, LynchResult) {
    let alive: Vec<PlayerId> = players.iter().map(|p| p.id).collect();
    let mut batch = BatchBuilder::default();
    let mut day = DayAnswers {
        votes: LynchVotes::new(rules, rng, alive.len()),
        abilities: vec![],
        peace: false,
        shots: vec![],
        fired: vec![],
        owners: vec![],
        players,
        reveal: rules.show_roles_on_death,
    };
    let mut asked = vec![];
    for player in players {
        day.votes.ask(&mut batch, player, &alive);
        let state = states.entry(player.id).or_default();
        let others: Vec<PlayerId> = alive.iter().copied().filter(|&id| id != player.id).collect();
        let question = def(player.role).day;
        let prompt = match question {
            DayQuestion::Pick(prompt) => Some(prompt),
            // A Gunner with no bullets left is not asked (Werewolf.cs:5102).
            DayQuestion::Shoot => match crate::roles::gunner::bullets_left(state) {
                0 => None,
                bullets => Some(Prompt::GunnerShoot { bullets }),
            },
            DayQuestion::None | DayQuestion::Ability { .. } => None,
        };
        let target = prompt.map(|prompt| batch.targets(player.id, prompt, &others, 1));
        if let (Some(index), DayQuestion::Shoot) = (target, question) {
            day.shots.push((index, *player));
        }
        // Each yes/no ability is used once, and not offered again
        // (Werewolf.cs:5070-5100, `!HasUsedAbility`).
        let toggle = match question {
            DayQuestion::Ability { prompt, .. } if !state.primary_used => Some(batch.toggle(player.id, prompt)),
            _ => None,
        };
        if let Some(index) = toggle {
            day.abilities.push((index, *player));
        }
        asked.push((player, target, toggle));
    }
    day.owners = batch.questions.iter().map(Question::player).collect();
    let answers = ask_phase(presenter, Phase::Day, &batch.questions, &mut day).await;
    let peace = day.peace;
    let shot = day.votes.excluded.clone();
    let fired = day.fired.clone();
    let votes_by_voter = day.votes.cast;

    let mut day_actions = vec![];
    for (player, target, toggle) in asked {
        let behavior = def(player.role).behavior;
        let state = states.entry(player.id).or_default();
        let ctx = DayContext {
            alive: &alive,
            self_id: player.id,
            // A shot was told as it was fired; someone shot today doesn't
            // snoop at day's end.
            chosen_target: single_target_answer(&answers, target).filter(|_| match player.role {
                Role::Gunner => fired.contains(&player.id),
                _ => !shot.contains(&player.id),
            }),
            toggle_choice: toggle_answer(&answers, toggle),
        };
        day_actions.extend(behavior.day_action(&ctx, state));
    }
    // Whichever of peace and trouble was answered last is the one that
    // stands (see `last_day_veto`), whatever order the players are in.
    let winner = if peace { DayAction::Pacify } else { DayAction::Trouble };
    if let Some(at) = day_actions.iter().position(|a| *a == winner) {
        let veto = day_actions.remove(at);
        day_actions.push(veto);
    }
    if peace {
        return (day_actions, LynchResult::NoVotes);
    }

    if let Some(results) = secret_lynch_results(&votes_by_voter, players, states, rules) {
        presenter.narrate(NarrationEvent::Announce(results)).await;
    }
    (day_actions, tally_lynch(&votes_by_voter, players, states))
}

/// Whichever of Pacify and Trouble was taken last today, if either. Each
/// cancels the other in the legacy callbacks, so the later one wins:
/// peace clears `_doubleLynch` (Werewolf.cs:917) and trouble clears
/// `_pacifistUsed` (Werewolf.cs:971). `day_actions` is in the order the
/// players acted.
pub fn last_day_veto(day_actions: &[DayAction]) -> Option<DayAction> {
    day_actions
        .iter()
        .rev()
        .find(|a| matches!(a, DayAction::Pacify | DayAction::Trouble))
        .copied()
}

/// Turns resolved day decisions into actual deaths. Two things can cancel
/// the lynch entirely:
///
/// - Pacifist's `Pacify` action vetoes the *entire* day's lynch
///   (Werewolf.cs:913-925) — checked first, since it overrides everything,
///   unless a Troublemaker acted after it (see `last_day_veto`).
/// - Prince's first lynch is survived, not fatal (Werewolf.cs:2745-2751:
///   `!lynched.HasUsedAbility`). This needs `lynch_target_role` (who the
///   target actually is) and `states` (to check/spend the one-time
///   immunity via `RoleState::primary_used`).
///
/// Gunner's `Shoot` target dies of `KillMethod::Shoot` independently of
/// either check above. `SpreadSilver`/`Reveal` have no death consequence.
pub fn apply_day_results(
    day_actions: &[DayAction],
    lynch_target: Option<PlayerId>,
    lynch_target_role: Option<Role>,
    states: &mut HashMap<PlayerId, RoleState>,
) -> Vec<(PlayerId, KillMethod)> {
    let mut deaths = vec![];

    let pacified = last_day_veto(day_actions) == Some(DayAction::Pacify);

    if !pacified {
        if let Some(target) = lynch_target {
            let prince_has_immunity =
                lynch_target_role == Some(Role::Prince) && !states.entry(target).or_default().primary_used;
            if prince_has_immunity {
                states.entry(target).or_default().primary_used = true;
            } else {
                deaths.push((target, KillMethod::Lynch));
            }
        }
    }

    for action in day_actions {
        match action {
            DayAction::Shoot { target, .. } => deaths.push((*target, KillMethod::Shoot)),
            DayAction::SpreadSilver
            | DayAction::Reveal
            | DayAction::Pacify
            | DayAction::Trouble
            | DayAction::Detonate { .. }
            | DayAction::Snoop { .. }
            | DayAction::Sleep => {}
        }
    }

    deaths
}

#[cfg(test)]
mod apply_day_results_tests {
    use super::*;

    #[test]
    fn a_lynched_prince_survives_his_first_lynch() {
        let prince = PlayerId(1);
        let mut states = HashMap::new();
        let deaths = apply_day_results(&[], Some(prince), Some(Role::Prince), &mut states);
        assert_eq!(deaths, vec![], "Prince's first lynch should not kill him");
        assert!(
            states.get(&prince).unwrap().primary_used,
            "immunity should be spent even though he survived"
        );
    }

    #[test]
    fn a_second_lynched_prince_dies() {
        let prince = PlayerId(1);
        let mut states = HashMap::new();
        states.insert(
            prince,
            RoleState {
                primary_used: true, // already spent his one save
                ..Default::default()
            },
        );
        let deaths = apply_day_results(&[], Some(prince), Some(Role::Prince), &mut states);
        assert_eq!(deaths, vec![(prince, KillMethod::Lynch)]);
    }

    #[test]
    fn pacify_vetoes_the_lynch_entirely() {
        let target = PlayerId(1);
        let mut states = HashMap::new();
        let deaths = apply_day_results(&[DayAction::Pacify], Some(target), Some(Role::Villager), &mut states);
        assert_eq!(deaths, vec![], "Pacifist's veto should cancel the lynch");
    }

    #[test]
    fn pacify_does_not_cancel_a_gunner_shot() {
        let lynch_target = PlayerId(1);
        let shot_target = PlayerId(2);
        let mut states = HashMap::new();
        let deaths = apply_day_results(
            &[
                DayAction::Pacify,
                DayAction::Shoot {
                    shooter: PlayerId(99),
                    target: shot_target,
                },
            ],
            Some(lynch_target),
            Some(Role::Villager),
            &mut states,
        );
        assert_eq!(deaths, vec![(shot_target, KillMethod::Shoot)]);
    }

    /// "Trouble overrides peace" (Werewolf.cs:971) — a same-day
    /// Troublemaker veto beats a Pacifist veto, so the lynch still lands.
    #[test]
    fn trouble_overrides_a_same_day_pacify() {
        let target = PlayerId(1);
        let mut states = HashMap::new();
        let deaths = apply_day_results(
            &[DayAction::Pacify, DayAction::Trouble],
            Some(target),
            Some(Role::Villager),
            &mut states,
        );
        assert_eq!(deaths, vec![(target, KillMethod::Lynch)]);
    }

    fn pair(a: u64, b: u64) -> (PlayerId, PlayerId) {
        (PlayerId(a), PlayerId(b))
    }

    /// Werewolf.cs:2651: the Mayor's vote keeps counting twice on every
    /// lynch after the reveal, not just the day of it.
    #[test]
    fn a_revealed_mayor_counts_twice_on_later_days() {
        let players = vec![
            AlivePlayer {
                id: PlayerId(1),
                role: Role::Mayor,
            },
            AlivePlayer {
                id: PlayerId(2),
                role: Role::Villager,
            },
            AlivePlayer {
                id: PlayerId(3),
                role: Role::Villager,
            },
        ];
        let mut states = HashMap::new();
        let votes = [pair(1, 2), pair(2, 3)];
        assert_eq!(tally_lynch(&votes, &players, &states), LynchResult::Tie);
        states.insert(
            PlayerId(1),
            RoleState {
                primary_used: true,
                ..Default::default()
            },
        );
        assert_eq!(
            tally_lynch(&votes, &players, &states),
            LynchResult::Lynched(PlayerId(2))
        );
        assert_eq!(tally_lynch(&[], &players, &states), LynchResult::NoVotes);
    }

    /// Werewolf.cs:999-1011: half the time the Clumsy Guy's vote lands on
    /// a random living player other than themselves.
    #[test]
    fn a_clumsy_guys_vote_can_go_astray() {
        let clumsy = AlivePlayer {
            id: PlayerId(1),
            role: Role::ClumsyGuy,
        };
        let alive = [PlayerId(1), PlayerId(2), PlayerId(3)];
        let rules = GameRules::default();
        let cast = |rng: &mut dyn RngSource| {
            let mut votes = LynchVotes::new(&rules, rng, alive.len());
            let mut batch = BatchBuilder::default();
            votes.ask(&mut batch, &clumsy, &alive);
            let said = votes.answered(0, &Answer::Targets(Some(vec![PlayerId(2)])));
            (votes.cast, said)
        };
        let mut rng = crate::rng::ScriptedRngSource::new().with_chance(true).with_index(1);
        let (votes, said) = cast(&mut rng);
        assert_eq!(votes, vec![(PlayerId(1), PlayerId(3))]);
        // The announcement names where the vote landed.
        assert_eq!(
            said,
            vec![NarrationEvent::Announce(Announcement::PlayerVotedLynch {
                voter: PlayerId(1),
                target: PlayerId(3)
            })]
        );
        let mut rng = crate::rng::ScriptedRngSource::new().with_chance(false);
        assert_eq!(cast(&mut rng).0, vec![(PlayerId(1), PlayerId(2))]);
    }

    /// A vote for yourself, or a second answer to the same ballot, is not
    /// counted.
    #[test]
    fn votes_count_once_and_never_for_yourself() {
        let voter = AlivePlayer {
            id: PlayerId(1),
            role: Role::Villager,
        };
        let alive = [PlayerId(1), PlayerId(2)];
        let rules = GameRules::default();
        let mut rng = crate::rng::ScriptedRngSource::new();
        let mut votes = LynchVotes::new(&rules, &mut rng, alive.len());
        let mut batch = BatchBuilder::default();
        votes.ask(&mut batch, &voter, &alive);
        assert!(votes.answered(0, &Answer::Targets(Some(vec![PlayerId(1)]))).is_empty());
        assert!(votes.answered(0, &Answer::Targets(Some(vec![PlayerId(2)]))).is_empty());
        assert!(votes.cast.is_empty());
    }

    /// Werewolf.cs:917 ("peace overrides trouble"): a Pacifist acting
    /// after the Troublemaker still cancels the lynch.
    #[test]
    fn pacify_after_trouble_cancels_the_lynch() {
        let target = PlayerId(1);
        let mut states = HashMap::new();
        let deaths = apply_day_results(
            &[DayAction::Trouble, DayAction::Pacify],
            Some(target),
            Some(Role::Villager),
            &mut states,
        );
        assert!(deaths.is_empty());
        assert_eq!(
            last_day_veto(&[DayAction::Trouble, DayAction::Pacify]),
            Some(DayAction::Pacify)
        );
    }
}

/// The Hunter's final shot (Werewolf.cs:5420-5484, `HunterFinalShot`).
/// Called by the game right after a batch of deaths, with the roster
/// already showing who survived it. Each Hunter in `died_with_roles` is
/// asked once, with a different prompt when lynched
/// (`killMethod == KillMthd.Lynch` in legacy), and whoever they pick dies
/// by `KillMethod::HunterShot`.
///
/// A Hunter eaten by the wolves, burned by the Arsonist or fallen into a
/// grave gets no shot: legacy passes `hunterFinalShot: false` there
/// (Werewolf.cs:3419, 3199, 2391). A Hunter who shoots back at the pack
/// does so in `night::Night::wolves_attack` instead.
///
/// Asks each dead Hunter in this batch once. "Domino" chains (a Hunter's
/// shot landing on *another* Hunter, Werewolf.cs:5480-5481) are handled by
/// `game::process_deaths_and_hunter_shots` calling this again on the
/// shots' own deaths.
/// Returns `(target, hunter)` pairs. Announces a
/// Hunter who shoots nobody (Werewolf.cs:5462) and one who shoots the
/// Wise Elder (Werewolf.cs:5473); a shot that kills is announced with the
/// death itself (`death_text`).
pub async fn resolve_hunter_shots(
    new_deaths: &[(PlayerId, KillMethod)],
    died_with_roles: &[(PlayerId, Role)],
    alive: &[AlivePlayer],
    presenter: &mut dyn Presenter,
) -> Vec<(PlayerId, PlayerId)> {
    let alive_ids: Vec<PlayerId> = alive.iter().map(|p| p.id).collect();
    let mut shots = vec![];

    for &(hunter_id, role) in died_with_roles {
        if role != Role::Hunter {
            continue;
        }
        let method = new_deaths
            .iter()
            .find(|&&(id, _)| id == hunter_id)
            .map(|&(_, m)| m)
            .expect("every id in died_with_roles has a matching entry in new_deaths");
        // Werewolf.cs passes `hunterFinalShot: false` for these (3419, 3199, 2391).
        if matches!(method, KillMethod::Eat | KillMethod::Burn | KillMethod::FallGrave) {
            continue;
        }
        let prompt = if method == KillMethod::Lynch {
            Prompt::HunterFinalShotLynched
        } else {
            Prompt::HunterFinalShotKilled
        };
        let options: Vec<PlayerId> = alive_ids.iter().copied().filter(|&id| id != hunter_id).collect();
        let lynched = method == KillMethod::Lynch;
        // Skip (an empty pick) and no answer are told apart
        // (Werewolf.cs:5453-5463).
        match presenter.ask_targets(hunter_id, prompt, &options, 1).await {
            Some(picked) if picked.is_empty() => {
                let announcement = Announcement::HunterSkipChoice {
                    hunter: hunter_id,
                    lynched,
                };
                presenter.narrate(NarrationEvent::Announce(announcement)).await;
            }
            picked => match picked.and_then(|p| validate_pick(p, &options, 1)) {
                Some(target) => shots.push((target[0], hunter_id)),
                None => {
                    let announcement = Announcement::HunterNoChoice {
                        hunter: hunter_id,
                        lynched,
                    };
                    presenter.narrate(NarrationEvent::Announce(announcement)).await;
                }
            },
        }
    }

    shots
}

/// Werewolf.cs's generic per-kill lover check, embedded in `KillPlayer`
/// itself (Werewolf.cs:5609-5616, `KillLover` at 5706-5715): whenever an
/// in-love player dies, their partner dies too (`KillMethod::LoverDied`),
/// unless the partner is already dying in this same batch — a mass-death
/// event (an Arsonist's burn catching both lovers at once, say) needs no
/// extra chained death, matching the legacy `!(dyingSimultaneously?
/// .Contains(x) ?? false)` guard.
///
/// Needs no question asked, unlike `resolve_hunter_shots` — this is
/// unconditional. And unlike a Hunter's shot (which could in principle
/// keep triggering more Hunters), this can't chain past one extra death:
/// Cupid links exactly one pair, so a lover's own death has no lover of
/// their own to chain into.
pub fn resolve_lover_deaths(
    new_deaths: &[(PlayerId, KillMethod)],
    lovers: &LoversState,
    alive: &[AlivePlayer],
) -> Vec<(PlayerId, KillMethod)> {
    let mut chain: Vec<(PlayerId, KillMethod)> = vec![];

    for &(victim, _) in new_deaths {
        let Some(partner) = lovers.partner_of(victim) else {
            continue;
        };
        let partner_alive = alive.iter().any(|p| p.id == partner);
        let already_dying = new_deaths.iter().any(|&(v, _)| v == partner) || chain.iter().any(|&(v, _)| v == partner);
        if partner_alive && !already_dying {
            chain.push((partner, KillMethod::LoverDied));
        }
    }

    chain
}

/// The percent chance a Grave Digger's trap catches whoever visits them
/// (Werewolf.cs:2389: `20 + (30 - 30 * 0.5^(dug-1))`, halved for a
/// Village-team visitor at 2390). Only called with `dug_graves >= 1` —
/// the real code short-circuits to a guaranteed-safe visit otherwise
/// (2380: `if (visited.DugGravesLastNight < 1) return VisitResult.Success`),
/// modeled by the caller never invoking this at all rather than this
/// function handling a `0` specially.
///
/// C#'s `Program.R.Next(100) < fallChance` compares an integer roll
/// against a `double`, so a fractional `fallChance` (e.g. `45.5`) is
/// meaningfully more generous than either whole neighbor. `RngSource`
/// only takes an integer percent, so this rounds to the nearest whole
/// percent rather than truncating — closer to the real odds than always
/// rounding down, and simple enough not to need its own `RngSource`
/// method just for one formula.
pub(crate) fn grave_digger_fall_chance_percent(dug_graves: u32, visitor_is_village: bool) -> u32 {
    let chance = 20.0 + (30.0 - 30.0 * 0.5_f64.powi(dug_graves as i32 - 1));
    let chance = if visitor_is_village { chance / 2.0 } else { chance };
    chance.round() as u32
}

/// Spumpkin's day-phase detonation (Werewolf.cs:2901-2930): a 40% chance
/// (Werewolf.cs:2907, no named `Settings` constant — a bare literal in
/// the source itself) of killing both the target (`KillMthd.Shoot` — yes,
/// the same method Gunner's shot uses, confirmed directly:
/// `KillPlayer(check, KillMthd.Shoot, killer: spumpkin, ...)`) and the
/// Spumpkin themself.
///
/// The real code passes a `null` `killMethod` for the Spumpkin's own
/// death (`KillPlayer(spumpkin, killMethod: null, killer: null, ...)`),
/// which in the source only affects whether a DB kill row gets written
/// and whether a Hunter final shot could trigger — irrelevant here (the
/// engine writes no kill rows, and a Spumpkin isn't a Hunter), so both deaths are given the same `KillMethod::Shoot` rather
/// than inventing a "no method" case this crate's `(PlayerId, KillMethod)`
/// death shape has no room for.
///
/// The WiseElder-demotion special case (Werewolf.cs:2916: detonating a
/// Wise Elder transforms the *Spumpkin* to Villager first) doesn't change
/// who dies — `KillPlayer(spumpkin, ...)` and `KillPlayer(check, ...)`
/// both still run unconditionally right after, regardless of which
/// branch of that `switch` fired — so it is achievement bookkeeping
/// that does not change who dies.
pub fn resolve_spumpkin_detonation(day_actions: &[DayAction], rng: &mut dyn RngSource) -> Vec<(PlayerId, KillMethod)> {
    day_actions
        .iter()
        .filter_map(|a| match a {
            DayAction::Detonate { spumpkin, target } if rng.chance(40) => {
                Some([(*spumpkin, KillMethod::Shoot), (*target, KillMethod::Shoot)])
            }
            _ => None,
        })
        .flatten()
        .collect()
}

#[cfg(test)]
mod hunter_shot_tests {
    use super::*;
    use async_trait::async_trait;

    /// Records the prompt/options it was asked, and answers with a fixed
    /// target (or declines if `answer` is `None`) — enough to prove both
    /// *which* question got asked and that the answer becomes a shot.
    struct ScriptedHunterPresenter {
        answer: Option<PlayerId>,
        asked: Vec<(PlayerId, Prompt, Vec<PlayerId>)>,
    }

    #[async_trait(?Send)]
    impl Presenter for ScriptedHunterPresenter {
        async fn ask_targets(
            &mut self,
            player: PlayerId,
            prompt: Prompt,
            options: &[PlayerId],
            count: usize,
        ) -> Option<Vec<PlayerId>> {
            self.asked.push((player, prompt, options.to_vec()));
            if count != 1 {
                return None;
            }
            self.answer.map(|t| vec![t])
        }
    }

    async fn shots(
        new_deaths: &[(PlayerId, KillMethod)],
        died_with_roles: &[(PlayerId, Role)],
        presenter: &mut ScriptedHunterPresenter,
    ) -> Vec<(PlayerId, KillMethod)> {
        resolve_hunter_shots(new_deaths, died_with_roles, &alive_roster(), presenter)
            .await
            .into_iter()
            .map(|(target, _)| (target, KillMethod::HunterShot))
            .collect()
    }

    fn alive_roster() -> Vec<AlivePlayer> {
        vec![
            AlivePlayer {
                id: PlayerId(2),
                role: Role::Villager,
            },
            AlivePlayer {
                id: PlayerId(3),
                role: Role::Seer,
            },
        ]
    }

    #[tokio::test]
    async fn a_lynched_hunter_is_asked_the_lynched_prompt_and_shoots() {
        let hunter = PlayerId(1);
        let target = PlayerId(2);
        let mut presenter = ScriptedHunterPresenter {
            answer: Some(target),
            asked: vec![],
        };
        let new_deaths = [(hunter, KillMethod::Lynch)];
        let died_with_roles = [(hunter, Role::Hunter)];

        let shots = shots(&new_deaths, &died_with_roles, &mut presenter).await;

        assert_eq!(shots, vec![(target, KillMethod::HunterShot)]);
        assert_eq!(presenter.asked.len(), 1);
        assert_eq!(presenter.asked[0].0, hunter);
        assert_eq!(presenter.asked[0].1, Prompt::HunterFinalShotLynched);
        assert!(
            !presenter.asked[0].2.contains(&hunter),
            "the dead hunter should never be offered as their own target"
        );
    }

    #[tokio::test]
    async fn a_hunter_killed_any_other_way_gets_the_killed_prompt() {
        let hunter = PlayerId(1);
        let mut presenter = ScriptedHunterPresenter {
            answer: None,
            asked: vec![],
        };
        let new_deaths = [(hunter, KillMethod::Poison)];
        let died_with_roles = [(hunter, Role::Hunter)];

        shots(&new_deaths, &died_with_roles, &mut presenter).await;

        assert_eq!(presenter.asked[0].1, Prompt::HunterFinalShotKilled);
    }

    /// Werewolf.cs:3419: the default wolf-eat kill explicitly passes
    /// `hunterFinalShot: false` — a plain wolf-eat death never grants a
    /// revenge shot, unlike every other kill method.
    #[tokio::test]
    async fn a_hunter_eaten_by_wolves_gets_no_final_shot_at_all() {
        let hunter = PlayerId(1);
        let mut presenter = ScriptedHunterPresenter {
            answer: Some(PlayerId(2)),
            asked: vec![],
        };
        let new_deaths = [(hunter, KillMethod::Eat)];
        let died_with_roles = [(hunter, Role::Hunter)];

        let shots = shots(&new_deaths, &died_with_roles, &mut presenter).await;

        assert_eq!(shots, vec![]);
        assert!(
            presenter.asked.is_empty(),
            "a wolf-eaten hunter should never be asked for a final shot"
        );
    }

    #[tokio::test]
    async fn declining_the_final_shot_kills_nobody() {
        let hunter = PlayerId(1);
        let mut presenter = ScriptedHunterPresenter {
            answer: None,
            asked: vec![],
        };
        let new_deaths = [(hunter, KillMethod::Lynch)];
        let died_with_roles = [(hunter, Role::Hunter)];

        let shots = shots(&new_deaths, &died_with_roles, &mut presenter).await;

        assert_eq!(shots, vec![]);
    }

    #[tokio::test]
    async fn non_hunters_are_never_asked() {
        let villager = PlayerId(1);
        let mut presenter = ScriptedHunterPresenter {
            answer: Some(PlayerId(2)),
            asked: vec![],
        };
        let new_deaths = [(villager, KillMethod::Lynch)];
        let died_with_roles = [(villager, Role::Villager)];

        let shots = shots(&new_deaths, &died_with_roles, &mut presenter).await;

        assert_eq!(shots, vec![]);
        assert!(
            presenter.asked.is_empty(),
            "a non-Hunter death should never trigger a final-shot question"
        );
    }
}

#[cfg(test)]
mod lover_death_tests {
    use super::*;

    fn alive_pair() -> Vec<AlivePlayer> {
        vec![
            AlivePlayer {
                id: PlayerId(2),
                role: Role::Villager,
            },
            AlivePlayer {
                id: PlayerId(3),
                role: Role::Seer,
            },
        ]
    }

    #[test]
    fn a_dead_lover_takes_their_living_partner_with_them() {
        let victim = PlayerId(1);
        let partner = PlayerId(2);
        let mut lovers = LoversState::default();
        lovers.link(victim, partner);

        let chain = resolve_lover_deaths(&[(victim, KillMethod::Eat)], &lovers, &alive_pair());

        assert_eq!(chain, vec![(partner, KillMethod::LoverDied)]);
    }

    #[test]
    fn no_link_means_no_chained_death() {
        let victim = PlayerId(1);
        let lovers = LoversState::default();
        let chain = resolve_lover_deaths(&[(victim, KillMethod::Eat)], &lovers, &alive_pair());
        assert_eq!(chain, vec![]);
    }

    #[test]
    fn a_partner_already_dead_does_not_die_again() {
        let victim = PlayerId(1);
        let partner = PlayerId(99); // not in alive_pair()
        let mut lovers = LoversState::default();
        lovers.link(victim, partner);

        let chain = resolve_lover_deaths(&[(victim, KillMethod::Eat)], &lovers, &alive_pair());

        assert_eq!(chain, vec![], "a partner who's already dead can't die again");
    }

    #[test]
    fn lovers_dying_simultaneously_do_not_double_chain() {
        let victim = PlayerId(1);
        let partner = PlayerId(2);
        let mut lovers = LoversState::default();
        lovers.link(victim, partner);

        // Both already in the same batch (e.g. an Arsonist's mass burn) -
        // no extra death should be added for either.
        let chain = resolve_lover_deaths(
            &[(victim, KillMethod::Burn), (partner, KillMethod::Burn)],
            &lovers,
            &alive_pair(),
        );

        assert_eq!(chain, vec![]);
    }
}

#[cfg(test)]
mod spumpkin_detonation_tests {
    use super::*;
    use crate::rng::ScriptedRngSource;

    #[test]
    fn a_successful_detonation_kills_both() {
        let spumpkin = PlayerId(1);
        let target = PlayerId(2);
        let actions = [DayAction::Detonate { spumpkin, target }];
        let mut rng = ScriptedRngSource::new().with_chance(true);

        let deaths = resolve_spumpkin_detonation(&actions, &mut rng);

        assert_eq!(deaths, vec![(spumpkin, KillMethod::Shoot), (target, KillMethod::Shoot)]);
    }

    #[test]
    fn a_failed_detonation_kills_nobody() {
        let spumpkin = PlayerId(1);
        let target = PlayerId(2);
        let actions = [DayAction::Detonate { spumpkin, target }];
        let mut rng = ScriptedRngSource::new().with_chance(false);

        assert_eq!(resolve_spumpkin_detonation(&actions, &mut rng), vec![]);
    }

    #[test]
    fn no_detonate_action_means_no_roll_at_all() {
        let mut rng = ScriptedRngSource::new(); // panics if consulted
        assert_eq!(resolve_spumpkin_detonation(&[], &mut rng), vec![]);
    }
}

#[cfg(test)]
mod grave_digger_fall_tests {
    use super::*;

    #[test]
    fn fall_chance_matches_the_real_formula_at_one_dig() {
        // 20 + (30 - 30*0.5^0) = 20, halved for a Village-team visitor -> 10
        assert_eq!(grave_digger_fall_chance_percent(1, true), 10);
    }

    #[test]
    fn fall_chance_matches_the_real_formula_at_two_digs() {
        // 20 + (30 - 30*0.5^1) = 35, halved -> 17.5, rounded -> 18
        assert_eq!(grave_digger_fall_chance_percent(2, true), 18);
    }

    #[test]
    fn fall_chance_is_not_halved_for_a_non_village_visitor() {
        assert_eq!(grave_digger_fall_chance_percent(1, false), 20);
    }

    #[test]
    fn fall_chance_approaches_fifty_percent_for_many_digs() {
        let chance = grave_digger_fall_chance_percent(20, false);
        assert!((49..=50).contains(&chance), "expected ~50, got {chance}");
    }
}

#[cfg(test)]
mod day_tests {
    use super::*;
    use crate::roles::DayAction;
    use async_trait::async_trait;
    use shared::KillMethod;

    struct ScriptedDayPresenter {
        lynch_votes: HashMap<PlayerId, PlayerId>,
        gunner_shot: Option<PlayerId>,
        mayor_reveals: bool,
    }

    #[async_trait(?Send)]
    impl Presenter for ScriptedDayPresenter {
        async fn ask_targets(
            &mut self,
            player: PlayerId,
            prompt: Prompt,
            _options: &[PlayerId],
            count: usize,
        ) -> Option<Vec<PlayerId>> {
            if count != 1 {
                return None;
            }
            match prompt {
                Prompt::LynchVote => self.lynch_votes.get(&player).copied().map(|t| vec![t]),
                Prompt::GunnerShoot { .. } => self.gunner_shot.map(|t| vec![t]),
                _ => None,
            }
        }

        async fn ask_toggle(&mut self, _player: PlayerId, prompt: Prompt) -> bool {
            matches!(prompt, Prompt::MayorReveal) && self.mayor_reveals
        }
    }

    #[tokio::test]
    async fn majority_lynch_vote_resolves_and_kills() {
        let voter_a = PlayerId(1);
        let voter_b = PlayerId(2);
        let target = PlayerId(3);
        let players = vec![
            AlivePlayer {
                id: voter_a,
                role: Role::Villager,
            },
            AlivePlayer {
                id: voter_b,
                role: Role::Villager,
            },
            AlivePlayer {
                id: target,
                role: Role::Villager,
            },
        ];

        let mut votes = HashMap::new();
        votes.insert(voter_a, target);
        votes.insert(voter_b, target);
        let mut presenter = ScriptedDayPresenter {
            lynch_votes: votes,
            gunner_shot: None,
            mayor_reveals: false,
        };
        let mut states = HashMap::new();

        let (day_actions, lynch_target) = resolve_day(
            &players,
            &mut states,
            &GameRules::default(),
            &mut presenter,
            &mut crate::rng::ScriptedRngSource::new(),
        )
        .await;
        let lynch_target = lynch_target.target();
        assert_eq!(lynch_target, Some(target));

        let mut states2 = HashMap::new();
        let deaths = apply_day_results(&day_actions, lynch_target, Some(Role::Villager), &mut states2);
        assert_eq!(deaths, vec![(target, KillMethod::Lynch)]);
    }

    #[tokio::test]
    async fn tied_lynch_vote_kills_nobody() {
        let voter_a = PlayerId(1);
        let voter_b = PlayerId(2);
        let candidate_1 = PlayerId(3);
        let candidate_2 = PlayerId(4);
        let players = vec![
            AlivePlayer {
                id: voter_a,
                role: Role::Villager,
            },
            AlivePlayer {
                id: voter_b,
                role: Role::Villager,
            },
            AlivePlayer {
                id: candidate_1,
                role: Role::Villager,
            },
            AlivePlayer {
                id: candidate_2,
                role: Role::Villager,
            },
        ];

        let mut votes = HashMap::new();
        votes.insert(voter_a, candidate_1);
        votes.insert(voter_b, candidate_2);
        let mut presenter = ScriptedDayPresenter {
            lynch_votes: votes,
            gunner_shot: None,
            mayor_reveals: false,
        };
        let mut states = HashMap::new();

        let (day_actions, lynch_target) = resolve_day(
            &players,
            &mut states,
            &GameRules::default(),
            &mut presenter,
            &mut crate::rng::ScriptedRngSource::new(),
        )
        .await;
        let lynch_target = lynch_target.target();
        assert_eq!(lynch_target, None);
        let mut states2 = HashMap::new();
        assert_eq!(
            apply_day_results(&day_actions, lynch_target, None, &mut states2),
            vec![]
        );
    }

    #[tokio::test]
    async fn gunner_shot_and_lynch_both_produce_deaths_independently() {
        let gunner = PlayerId(1);
        let lynch_target = PlayerId(2);
        let shot_target = PlayerId(3);
        let players = vec![
            AlivePlayer {
                id: gunner,
                role: Role::Gunner,
            },
            AlivePlayer {
                id: lynch_target,
                role: Role::Villager,
            },
            AlivePlayer {
                id: shot_target,
                role: Role::Villager,
            },
        ];

        let mut votes = HashMap::new();
        votes.insert(gunner, lynch_target);
        let mut presenter = ScriptedDayPresenter {
            lynch_votes: votes,
            gunner_shot: Some(shot_target),
            mayor_reveals: false,
        };
        let mut states = HashMap::new();

        let (day_actions, lynch) = resolve_day(
            &players,
            &mut states,
            &GameRules::default(),
            &mut presenter,
            &mut crate::rng::ScriptedRngSource::new(),
        )
        .await;
        let lynch = lynch.target();
        assert_eq!(lynch, Some(lynch_target));
        assert!(day_actions
            .iter()
            .any(|a| matches!(a, DayAction::Shoot { target, .. } if *target == shot_target)));

        let mut states2 = HashMap::new();
        let mut deaths = apply_day_results(&day_actions, lynch, Some(Role::Villager), &mut states2);
        deaths.sort_by_key(|(id, _)| id.0);
        assert_eq!(
            deaths,
            vec![(lynch_target, KillMethod::Lynch), (shot_target, KillMethod::Shoot),]
        );
    }

    /// Without the reveal, this is a tie (one vote each) and nobody dies.
    /// The Mayor revealing doubles his own vote, breaking the tie in his
    /// candidate's favor — proving `resolve_day`'s vote-doubling actually
    /// changes the outcome rather than just being tracked and ignored.
    #[tokio::test]
    async fn a_revealed_mayor_s_doubled_vote_breaks_a_tie() {
        let mayor = PlayerId(1);
        let other_voter = PlayerId(2);
        let mayors_pick = PlayerId(3);
        let other_pick = PlayerId(4);
        let players = vec![
            AlivePlayer {
                id: mayor,
                role: Role::Mayor,
            },
            AlivePlayer {
                id: other_voter,
                role: Role::Villager,
            },
            AlivePlayer {
                id: mayors_pick,
                role: Role::Villager,
            },
            AlivePlayer {
                id: other_pick,
                role: Role::Villager,
            },
        ];

        let mut votes = HashMap::new();
        votes.insert(mayor, mayors_pick);
        votes.insert(other_voter, other_pick);
        let mut presenter = ScriptedDayPresenter {
            lynch_votes: votes,
            gunner_shot: None,
            mayor_reveals: true,
        };
        let mut states = HashMap::new();

        let (_day_actions, lynch_target) = resolve_day(
            &players,
            &mut states,
            &GameRules::default(),
            &mut presenter,
            &mut crate::rng::ScriptedRngSource::new(),
        )
        .await;
        let lynch_target = lynch_target.target();
        assert_eq!(
            lynch_target,
            Some(mayors_pick),
            "the Mayor's revealed vote should count twice and break the tie"
        );
    }

    /// Same votes as above, but the Mayor declines to reveal: no doubling,
    /// so it's a genuine tie and nobody is lynched.
    #[tokio::test]
    async fn an_unrevealed_mayor_s_vote_only_counts_once() {
        let mayor = PlayerId(1);
        let other_voter = PlayerId(2);
        let mayors_pick = PlayerId(3);
        let other_pick = PlayerId(4);
        let players = vec![
            AlivePlayer {
                id: mayor,
                role: Role::Mayor,
            },
            AlivePlayer {
                id: other_voter,
                role: Role::Villager,
            },
            AlivePlayer {
                id: mayors_pick,
                role: Role::Villager,
            },
            AlivePlayer {
                id: other_pick,
                role: Role::Villager,
            },
        ];

        let mut votes = HashMap::new();
        votes.insert(mayor, mayors_pick);
        votes.insert(other_voter, other_pick);
        let mut presenter = ScriptedDayPresenter {
            lynch_votes: votes,
            gunner_shot: None,
            mayor_reveals: false,
        };
        let mut states = HashMap::new();

        let (_day_actions, lynch_target) = resolve_day(
            &players,
            &mut states,
            &GameRules::default(),
            &mut presenter,
            &mut crate::rng::ScriptedRngSource::new(),
        )
        .await;
        let lynch_target = lynch_target.target();
        assert_eq!(lynch_target, None);
    }
}
