//! Ties the individual role files together into an actual night
//! resolution, without ever knowing where questions go or answers come
//! from. That's the `Presenter` trait: the orchestrator calls
//! `ask_targets` and gets answers back, and has no idea whether it's
//! talking to a real Telegram chat or a scripted test harness. `sim` and
//! (eventually) `control` each implement `Presenter` their own way; this
//! file never depends on either.
//!
//! `Presenter` has exactly one question-asking method, parameterized by
//! how many targets are wanted, rather than one method per arity
//! (`ask_target`, `ask_two_targets`, ...). A one-per-arity design is fine
//! for exactly one extra case, but it doesn't stop there — the moment a
//! third arity shows up (a role linking three players, say), every
//! `Presenter` implementation would need a matching new method forever.
//! Collapsing to `ask_targets(.., count)` means "how many" is just a
//! number, and implementers (a real Telegram adapter, a test double)
//! never need to know or care how many roles-that-pick-N-players exist.
//!
//! Resolution happens in dependency order, derived from
//! `RoleBehavior::requires()` rather than a hardcoded phase list: every
//! role whose dependencies are empty is asked in one batch first, then
//! anything depending on a fact produced by that batch (currently just
//! `NightFact::WolfTarget`, produced by tallying `NightAction::EatVote`)
//! is asked in a second batch with that fact filled in. This is
//! deliberately only two levels deep for now — there's exactly one
//! dependent role (`Witch`) and one fact — generalizing to a real N-level
//! topological sort is future work for whenever a second dependency shows
//! up and two hardcoded levels stop being enough.

use crate::rng::RngSource;
use crate::roles::{
    behavior_for, DayAction, DayContext, LoversState, NightAction, NightContext, NightFact,
    PlayerId, RoleState,
};
use async_trait::async_trait;
use shared::{KillMethod, Role, Team};
use std::collections::HashMap;

/// Identifies *why* a player is being asked something, so a presenter can
/// look up the right prompt text (via `i18n`) without the orchestrator
/// needing to know anything about message formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Prompt {
    WolfEat,
    SeerCheck,
    HarlotVisit,
    GuardianAngelProtect,
    DetectiveInvestigate,
    FoolInvestigate,
    WildChildRoleModel,
    CupidLink,
    WitchHeal,
    WitchPoison,
    /// Universal — every alive player is asked this, not just one role.
    /// See `resolve_day`.
    LynchVote,
    GunnerShoot,
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
}

/// Maps a `Prompt` to the real `Languages/English.xml` key the legacy game
/// used for that exact menu, where one exists. Colocated with `Prompt`
/// itself (not left to whichever `Presenter` happens to need it first)
/// because "what does this identifier mean, in the legacy game's own
/// words" is a fact about the identifier, not a presentation choice — a
/// real Telegram presenter and a text-transcript presenter should both
/// read it from here rather than each maintaining their own copy that can
/// silently drift out of sync with each other. `None` means there's no
/// exact legacy key (a new role not in the original game, or a menu this
/// proof-of-concept's `Prompt` doesn't line up 1:1 with) — callers decide
/// their own fallback wording, that part **is** a presentation choice.
///
/// Verified directly against `Languages/English.xml` and Werewolf.cs's
/// `SendNightActions`/`SendDayActions` switches, not guessed — an earlier
/// version of this table (before it lived here) wrongly assumed
/// Fool/Sorcerer/Oracle had no matching key, when they actually reuse
/// Seer's `AskSee` verbatim (Werewolf.cs:5174-5178).
pub fn prompt_locale_key(prompt: Prompt) -> Option<&'static str> {
    match prompt {
        Prompt::WolfEat => Some("AskEat"),
        Prompt::SeerCheck
        | Prompt::FoolInvestigate
        | Prompt::SorcererInvestigate
        | Prompt::OracleInvestigate => Some("AskSee"),
        Prompt::HarlotVisit => Some("AskVisit"),
        Prompt::GuardianAngelProtect => Some("AskGuard"),
        Prompt::DetectiveInvestigate => Some("AskDetect"),
        Prompt::CultistHunterInvestigate => Some("AskHunt"),
        Prompt::WildChildRoleModel => Some("AskRoleModel"),
        Prompt::CupidLink => Some("AskCupid1"),
        Prompt::LynchVote => Some("AskLynch"),
        Prompt::GunnerShoot => Some("AskShoot"),
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
        // WitchHeal/WitchPoison belong to a role that isn't in the legacy
        // game at all. BlacksmithSilver and SpumpkinDetonate are
        // menu-driven in Werewolf.cs but have no corresponding key in
        // English.xml (checked directly, not an oversight).
        Prompt::WitchHeal | Prompt::WitchPoison | Prompt::BlacksmithSilver | Prompt::SpumpkinDetonate => None,
    }
}

/// Every `Prompt` variant, in one place, so both the completeness test
/// (`sim/tests/locale_coverage.rs`) and anything else that needs "all of
/// them" (a future exhaustive UI, say) can iterate without re-deriving the
/// list by hand and risking it drifting from the enum above.
pub const ALL_PROMPTS: &[Prompt] = &[
    Prompt::WolfEat,
    Prompt::SeerCheck,
    Prompt::HarlotVisit,
    Prompt::GuardianAngelProtect,
    Prompt::DetectiveInvestigate,
    Prompt::FoolInvestigate,
    Prompt::WildChildRoleModel,
    Prompt::CupidLink,
    Prompt::WitchHeal,
    Prompt::WitchPoison,
    Prompt::LynchVote,
    Prompt::GunnerShoot,
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
    },
    /// A role transform resolved (Werewolf.cs's `Transform(...)` calls) —
    /// Traitor becoming a Wolf, Doppelganger copying a dead role model,
    /// etc. `from`/`to` rather than just `to` because several legacy
    /// messages (`DGToWolf`, `DGToCult`, ...) are phrased around what the
    /// player *was*, not just what they became.
    Transform {
        player: PlayerId,
        from: Role,
        to: Role,
    },
    /// The game has ended. Carries the whole `WinOutcome`, not just a
    /// `Team`, so a presenter can tell a genuine "no winner yet" (never
    /// produced — `run_game` only emits this once it actually returns)
    /// apart from the honest non-`Team` cases (`RngDependent`,
    /// `InsufficientData`, `Unimplemented`) this proof-of-concept
    /// sometimes has to report instead of a clean win.
    GameOver { winner: crate::WinOutcome },
}

/// Maps a death to the real legacy flavor key, where this proof-of-concept
/// has one.
///
/// The legacy `*Killed` keys (`GunnerKilled`, `SeerKilled`, `PrinceKilled`,
/// ...) are all Serial-Killer-specific flavor text, not general per-role
/// death messages (confirmed by reading their actual English.xml values —
/// every one mentions "the serial killer has struck again"), so they're
/// only used for `KillMethod::SerialKilled` specifically, never for a
/// lynch/eat/etc. death of the same role — `DefaultKilled` covers every
/// role without its own dedicated key. `KillMethod::VisitWolf` (a Harlot
/// visiting the wolves' actual target, see `resolve_harlot_visit_deaths`)
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
/// `KillMethod::Eat`'s per-role flavor (Werewolf.cs:4218-4267, the
/// `p.KilledByRole == IRole.Wolf && !p.DiedByVisitingKiller &&
/// !p.DiedByVisitingVictim` branch) is a *different* switch from the
/// `SerialKilled` one below — ten roles get their own `"{Role}Eaten"` key
/// (`ApprenticeSeer`, `Detective`, `Drunk`, `Fool`, `Gunner`, `Harlot`,
/// `Mason`, `Seer`, `Sorcerer`, `WildChild`), `GuardianAngel` gets
/// `"GuardianEaten"` (not `"GuardianAngelEaten"` — checked, that key
/// doesn't exist), and everyone else falls through to `"DefaultEaten"`.
/// Every one of these is reachable today: `apply_night_results` already
/// produces plain `KillMethod::Eat` deaths for the main wolf-eat and
/// Wolf Cub's bonus kill alike, and neither is a "visiting" death (that's
/// `VisitWolf`, the Harlot's own separate case below), matching this
/// branch's guard exactly.
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
        // The Harlot visiting the Serial Killer player themself
        // (Werewolf.cs:2339-2345 → the same switch's `KilledByRole ==
        // SerialKiller` + `DiedByVisitingKiller` case, 4368-4369).
        KillMethod::VisitKiller => Some("HarlotFuckKillerPublic"),
        // The Harlot visiting whoever the Serial Killer separately killed
        // that same night — a different player from the SK themself
        // (Werewolf.cs:3893-3897 → `KilledByRole == SerialKiller` +
        // `DiedByVisitingVictim`, 4370-4371).
        KillMethod::VisitVictim => Some("HarlotFuckedKilledPublic"),
        KillMethod::Eat => Some(match role {
            Role::ApprenticeSeer => "ApprenticeSeerEaten",
            Role::Detective => "DetectiveEaten",
            Role::Drunk => "DrunkEaten",
            Role::Fool => "FoolEaten",
            Role::Gunner => "GunnerEaten",
            Role::Harlot => "HarlotEaten",
            Role::Mason => "MasonEaten",
            Role::Seer => "SeerEaten",
            Role::Sorcerer => "SorcererEaten",
            Role::WildChild => "WildChildEaten",
            Role::GuardianAngel => "GuardianEaten",
            _ => "DefaultEaten",
        }),
        // Werewolf.cs:4251-4262. `Cupid` belongs in this list too (it was
        // missing before — checked directly against the real switch,
        // which includes `IRole.Cupid` alongside `Blacksmith`/`Cultist`/
        // `Drunk`/`GuardianAngel`/`Gunner`/`Mayor`/`Prince`/`Seer`).
        KillMethod::SerialKilled => Some(match role {
            Role::Gunner => "GunnerKilled",
            Role::Seer => "SeerKilled",
            Role::Drunk => "DrunkKilled",
            Role::Mayor => "MayorKilled",
            Role::Prince => "PrinceKilled",
            Role::Cultist => "CultistKilled",
            Role::GuardianAngel => "GuardianAngelKilled",
            Role::Blacksmith => "BlacksmithKilled",
            Role::Cupid => "CupidKilled",
            _ => "DefaultKilled",
        }),
        // Werewolf.cs:4288-4302, the "killed by visiting Grave Digger"
        // switch. Wolf-team/Cultist/Thief keys are listed for completeness
        // even though `resolve_grave_digger_falls` can't produce them yet
        // (see that function's doc for why) - a real, verified mapping
        // that's momentarily unreachable is still worth having ready for
        // when the wolf-visitor-identity gap closes, unlike guessing at a
        // key that doesn't exist.
        KillMethod::FallGrave => Some(match role {
            Role::Wolf | Role::AlphaWolf | Role::Lycan | Role::WolfCub => "WolfFellPublic",
            Role::Harlot => "HarlotFellPublic",
            Role::Cultist => "CultistFellPublic",
            Role::Thief => "ThiefFellPublic",
            _ => "DefaultFellPublic",
        }),
        // `resolve_chemistry` produces this for either the target (success)
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
        _ => None,
    }
}

/// Maps a resolved transform to its real legacy flavor key. Covers the
/// transforms `game::apply_transforms`/`demote_gunner_if_shot_a_wise_elder`
/// actually model: Traitor→Wolf (`TraitorTurnWolf`), ApprenticeSeer→Seer
/// (`ApprenticeNowSeer`), WildChild→Wolf (`WildChildTransform`),
/// Doppelganger copying a dead role model into one of the four roles the
/// legacy game has dedicated flavor text for (`DGToWolf`/`DGToCult`/
/// `DGToMason`/`DGToSnowWolf` — a Doppelganger copying any other role has
/// no dedicated key, checked directly), and a Gunner demoted for shooting
/// the Wise Elder (`GunnerShotWiseElder`). That last one is matched on
/// `(Gunner, Villager)` alone, not some more specific "reason" tag — safe
/// today because nothing else in this proof-of-concept produces that
/// exact transform, but a second cause of a Gunner becoming a Villager
/// would need `NarrationEvent::Transform` to carry a cause, not just
/// `from`/`to`, to disambiguate correctly.
pub fn transform_locale_key(from: Role, to: Role) -> Option<&'static str> {
    match (from, to) {
        (Role::Traitor, Role::Wolf) => Some("TraitorTurnWolf"),
        (Role::ApprenticeSeer, Role::Seer) => Some("ApprenticeNowSeer"),
        (Role::WildChild, Role::Wolf) => Some("WildChildTransform"),
        (Role::Doppelganger, Role::Wolf) => Some("DGToWolf"),
        (Role::Doppelganger, Role::Cultist) => Some("DGToCult"),
        (Role::Doppelganger, Role::Mason) => Some("DGToMason"),
        (Role::Doppelganger, Role::SnowWolf) => Some("DGToSnowWolf"),
        (Role::Gunner, Role::Villager) => Some("GunnerShotWiseElder"),
        // The cult's recruitment message (Werewolf.cs:2063: `Send
        // (GetLocaleString("CultConvertYou"), p.Id)`) doesn't vary by what
        // the convert used to be - a genuine `(_, Cultist)` wildcard, not
        // a missing case list. `(Doppelganger, Cultist)` still needs its
        // own more specific `DGToCult` above (Rust match arms take the
        // first one that fits, so the specific arm has to come first) -
        // Doppelganger's own conversion flavor is different text
        // entirely, checked directly against Werewolf.cs:2165-2172.
        (_, Role::Cultist) => Some("CultConvertYou"),
        _ => None,
    }
}

/// Maps a game-ending `WinOutcome` to its real legacy per-team victory
/// key. Only defined for an actual `Team` win — the RNG/data-gap
/// `WinOutcome` variants (`RngDependent`, `InsufficientData`,
/// `Unimplemented`) have no legacy equivalent to look up, since the real
/// game always resolved to a concrete winner one way or another; that gap
/// is exactly what those variants exist to flag (see `WinOutcome`'s own
/// doc). `Team::NoOne`/`Team::Thief` also have no key — `Team::Thief` is
/// the Thief's transient pre-steal team, never itself a winning faction
/// in `evaluate_winner_with_kills` (checked: nothing produces
/// `WinOutcome::Team(Team::Thief)` today), and no "nobody wins" win
/// screen exists in English.xml either. Both are still named explicitly
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
        WinOutcome::Team(Team::NoOne)
        | WinOutcome::Team(Team::Thief)
        | WinOutcome::RngDependent(_)
        | WinOutcome::InsufficientData(_)
        | WinOutcome::Unimplemented(_) => None,
    }
}

/// Every distinct locale key `prompt_locale_key`/`death_locale_key` can
/// currently produce, in one place — the input to the locale-coverage
/// completeness check (`sim/tests/locale_coverage.rs`), which measures
/// "how much of English.xml does this port actually reach" as a real
/// number instead of a vibe. Grows every time a mapping table grows;
/// nothing computes this automatically today (there's no exhaustive
/// `Role`/`KillMethod` iterator to drive `death_locale_key` with yet, the
/// same "two hardcoded levels are enough until a second case shows up"
/// tradeoff as `resolve_night`'s dependency resolution), so this is
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
        // FallGrave's currently-*reachable* outputs only - "WolfFellPublic"/
        // "CultistFellPublic"/"ThiefFellPublic" are real, verified mappings
        // death_locale_key already returns for their roles, but
        // resolve_grave_digger_falls can't produce a FallGrave death for
        // any of those roles yet (see its doc), so they'd inflate this
        // count with something unreachable today - left out until the gap
        // closes, same discipline as everywhere else in this list.
        "HarlotFellPublic",
        "DefaultFellPublic",
        "ChemistSuccessPublic",
        "ChemistFailPublic",
        // transform_locale_key's non-`None` outputs.
        "TraitorTurnWolf",
        "ApprenticeNowSeer",
        "WildChildTransform",
        "DGToWolf",
        "DGToCult",
        "DGToMason",
        "DGToSnowWolf",
        "GunnerShotWiseElder",
        "CultConvertYou",
        // game_over_locale_key's non-`None` outputs.
        "VillageWins",
        "WolfWins",
        "TannerWins",
        "CultWins",
        "SerialKillerWins",
        "ArsonistWins",
        "LoversWin",
    ]);
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
    fn known_transforms_map_to_their_real_keys() {
        assert_eq!(transform_locale_key(Role::Traitor, Role::Wolf), Some("TraitorTurnWolf"));
        assert_eq!(transform_locale_key(Role::ApprenticeSeer, Role::Seer), Some("ApprenticeNowSeer"));
        assert_eq!(transform_locale_key(Role::WildChild, Role::Wolf), Some("WildChildTransform"));
        assert_eq!(transform_locale_key(Role::Doppelganger, Role::SnowWolf), Some("DGToSnowWolf"));
    }

    #[test]
    fn a_doppelganger_copying_an_uncelebrated_role_has_no_key() {
        assert_eq!(transform_locale_key(Role::Doppelganger, Role::Villager), None);
    }

    #[test]
    fn a_gunner_demoted_for_shooting_the_wise_elder_has_a_key() {
        assert_eq!(transform_locale_key(Role::Gunner, Role::Villager), Some("GunnerShotWiseElder"));
    }

    #[test]
    fn serial_killed_maps_to_the_dying_roles_own_flavor_key() {
        assert_eq!(death_locale_key(Role::Gunner, KillMethod::SerialKilled), Some("GunnerKilled"));
        assert_eq!(death_locale_key(Role::Seer, KillMethod::SerialKilled), Some("SeerKilled"));
        assert_eq!(death_locale_key(Role::Blacksmith, KillMethod::SerialKilled), Some("BlacksmithKilled"));
        assert_eq!(death_locale_key(Role::Cupid, KillMethod::SerialKilled), Some("CupidKilled"));
    }

    #[test]
    fn serial_killed_falls_back_to_default_for_an_uncelebrated_role() {
        assert_eq!(death_locale_key(Role::Villager, KillMethod::SerialKilled), Some("DefaultKilled"));
    }

    #[test]
    fn a_harlot_dying_from_visiting_a_wolf_has_a_key() {
        assert_eq!(death_locale_key(Role::Harlot, KillMethod::VisitWolf), Some("HarlotFuckedWolfPublic"));
    }

    #[test]
    fn wolf_eat_maps_to_the_dying_roles_own_flavor_key() {
        assert_eq!(death_locale_key(Role::Seer, KillMethod::Eat), Some("SeerEaten"));
        assert_eq!(death_locale_key(Role::Detective, KillMethod::Eat), Some("DetectiveEaten"));
        assert_eq!(death_locale_key(Role::GuardianAngel, KillMethod::Eat), Some("GuardianEaten"));
    }

    #[test]
    fn wolf_eat_falls_back_to_default_for_an_uncelebrated_role() {
        assert_eq!(death_locale_key(Role::Villager, KillMethod::Eat), Some("DefaultEaten"));
    }

    #[test]
    fn chemistry_success_and_failure_map_to_different_keys() {
        assert_eq!(death_locale_key(Role::Villager, KillMethod::Chemistry), Some("ChemistSuccessPublic"));
        assert_eq!(death_locale_key(Role::Chemist, KillMethod::Chemistry), Some("ChemistFailPublic"));
    }

    #[test]
    fn lover_died_and_hunter_shot_still_have_no_key() {
        assert_eq!(death_locale_key(Role::Villager, KillMethod::LoverDied), None);
        assert_eq!(death_locale_key(Role::Villager, KillMethod::HunterShot), None);
    }

    #[test]
    fn known_win_outcomes_map_to_their_real_keys() {
        assert_eq!(game_over_locale_key(&WinOutcome::Team(Team::Village)), Some("VillageWins"));
        assert_eq!(game_over_locale_key(&WinOutcome::Team(Team::Wolf)), Some("WolfWins"));
        assert_eq!(game_over_locale_key(&WinOutcome::Team(Team::Lovers)), Some("LoversWin"));
    }

    #[test]
    fn rng_dependent_and_no_one_outcomes_have_no_key() {
        assert_eq!(game_over_locale_key(&WinOutcome::Team(Team::NoOne)), None);
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
    /// rather than trusting a malformed response.
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
}

/// The one place that validates a presenter's answer actually matches what
/// was asked for: exactly `count` entries, all distinct. Every call site
/// below goes through this — including `ask_one` — so "what counts as a
/// well-formed answer" is defined once, not re-derived per arity.
async fn ask_exact(
    presenter: &mut dyn Presenter,
    player: PlayerId,
    prompt: Prompt,
    options: &[PlayerId],
    count: usize,
) -> Option<Vec<PlayerId>> {
    let picked = presenter.ask_targets(player, prompt, options, count).await?;
    let all_distinct = (1..picked.len()).all(|i| !picked[..i].contains(&picked[i]));
    if picked.len() == count && all_distinct {
        Some(picked)
    } else {
        None
    }
}

/// Convenience for the one-target case, since it's by far the common one
/// (every role in `one_target_prompt` below, plus both of Witch's potion
/// questions — nine call sites and counting). Cupid's two-target ask has
/// exactly one call site, so it isn't given the same treatment: naming a
/// function for something called once is the same mistake as `ask_target`/
/// `ask_two_targets` being separate trait methods, just moved down a
/// layer — it goes straight through `ask_exact` at its call site instead.
async fn ask_one(
    presenter: &mut dyn Presenter,
    player: PlayerId,
    prompt: Prompt,
    options: &[PlayerId],
) -> Option<PlayerId> {
    ask_exact(presenter, player, prompt, options, 1)
        .await
        .and_then(|v| v.into_iter().next())
}

/// What every alive player brings into either phase: who they are and
/// what role they're currently playing. Shared by `resolve_night` and
/// `resolve_day` — a player's identity and role don't change between the
/// two, only which questions get asked.
#[derive(Debug, Clone, Copy)]
pub struct AlivePlayer {
    pub id: PlayerId,
    pub role: Role,
}

/// Maps a role to the shape of question it needs asked, if any. Roles with
/// no entry here (Villager, Drunk, Traitor, Cursed, Tanner — see their
/// module docs) have no night question at all; `night_action` is still
/// called for them with an empty context, which is harmless since their
/// implementations don't look at it.
fn one_target_prompt(role: Role) -> Option<Prompt> {
    match role {
        Role::Wolf => Some(Prompt::WolfEat),
        Role::Seer => Some(Prompt::SeerCheck),
        Role::Harlot => Some(Prompt::HarlotVisit),
        Role::GuardianAngel => Some(Prompt::GuardianAngelProtect),
        Role::Detective => Some(Prompt::DetectiveInvestigate),
        Role::Fool => Some(Prompt::FoolInvestigate),
        Role::WildChild => Some(Prompt::WildChildRoleModel),
        Role::CultistHunter => Some(Prompt::CultistHunterInvestigate),
        Role::Sorcerer => Some(Prompt::SorcererInvestigate),
        Role::Oracle => Some(Prompt::OracleInvestigate),
        Role::Cultist => Some(Prompt::CultistConvert),
        Role::AlphaWolf | Role::WolfCub | Role::Lycan => Some(Prompt::WolfEat),
        Role::Thief => Some(Prompt::ThiefSteal),
        Role::Chemist => Some(Prompt::ChemistBrew),
        Role::Arsonist => Some(Prompt::ArsonistDouse),
        _ => None,
    }
}

/// Roles with a yes/no night decision instead of a target pick.
fn toggle_night_prompt(role: Role) -> Option<Prompt> {
    match role {
        Role::Sandman => Some(Prompt::SandmanSleep),
        Role::Arsonist => Some(Prompt::ArsonistSpark),
        _ => None,
    }
}

/// Resolves one full night: asks every independent role first, tallies
/// the wolves' target, then asks dependent roles (currently just Witch)
/// with that fact available. Returns every `NightAction` produced, plus
/// the resolved wolf target for the orchestrator's caller to apply deaths
/// with (that application step — actually killing someone — is future
/// work; this function only resolves *decisions*).
pub async fn resolve_night(
    players: &[AlivePlayer],
    states: &mut HashMap<PlayerId, RoleState>,
    presenter: &mut dyn Presenter,
) -> (Vec<NightAction>, Option<PlayerId>) {
    let alive: Vec<PlayerId> = players.iter().map(|p| p.id).collect();
    let mut actions = vec![];

    // --- Stage 1: roles with no dependencies ---
    for player in players {
        let behavior = behavior_for(player.role);
        if !behavior.requires().is_empty() {
            continue; // handled in stage 2
        }

        let state = states.entry(player.id).or_default();

        if player.role == Role::Cupid {
            let chosen = ask_exact(presenter, player.id, Prompt::CupidLink, &alive, 2)
                .await
                .map(|v| (v[0], v[1]));
            let ctx = NightContext {
                alive: &alive,
                self_id: player.id,
                chosen_target: None,
                heal_target: None,
                poison_target: None,
                love_targets: chosen,
                wolf_target: None,
                toggle_choice: false,
            };
            actions.extend(behavior.night_action(&ctx, state));
            continue;
        }

        let chosen_target = match one_target_prompt(player.role) {
            Some(prompt) => ask_one(presenter, player.id, prompt, &alive).await,
            None => None,
        };
        let toggle_choice = match toggle_night_prompt(player.role) {
            Some(prompt) => presenter.ask_toggle(player.id, prompt).await,
            None => false,
        };

        let ctx = NightContext {
            alive: &alive,
            self_id: player.id,
            chosen_target,
            heal_target: None,
            poison_target: None,
            love_targets: None,
            wolf_target: None,
            toggle_choice,
        };
        actions.extend(behavior.night_action(&ctx, state));
    }

    // Tally the wolves' target: majority vote among EatVote actions, tie
    // or no votes resolves to None. Real tie-breaking rules (if any) are
    // future work — this is enough to unblock stage 2's dependency.
    let wolf_target = majority_eat_target(&actions);

    // --- Stage 2: roles depending on a fact produced above ---
    for player in players {
        let behavior = behavior_for(player.role);
        if behavior.requires().is_empty() {
            continue; // already handled in stage 1
        }
        debug_assert!(
            behavior.requires().contains(&NightFact::WolfTarget),
            "stage 2 currently only understands WolfTarget-dependent roles"
        );

        let state = states.entry(player.id).or_default();

        // Witch is the only stage-2 role today: ask both her potion
        // questions. `is_available()` is a role-assignment-time concern
        // (whether she's dealt into the game at all), not enforced here —
        // if she's in `players`, the orchestrator resolves her correctly
        // regardless, which is what proves the dependency plumbing works.
        let heal_target = ask_one(presenter, player.id, Prompt::WitchHeal, &alive).await;
        let poison_target = ask_one(presenter, player.id, Prompt::WitchPoison, &alive).await;

        let ctx = NightContext {
            alive: &alive,
            self_id: player.id,
            chosen_target: None,
            heal_target,
            poison_target,
            love_targets: None,
            wolf_target,
            toggle_choice: false,
        };
        actions.extend(behavior.night_action(&ctx, state));
    }

    (actions, wolf_target)
}

/// The one place "tally votes, find the majority, ties resolve to no
/// winner" is defined. Both the wolves' eat vote and the day's lynch vote
/// need exactly this; it very nearly got copy-pasted a second time when
/// `resolve_day` was added, which is the same duplication mistake as
/// `ask_target`/`ask_two_targets` in miniature — caught before it existed
/// this time instead of after.
fn majority_target(votes: impl Iterator<Item = PlayerId>) -> Option<PlayerId> {
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

fn majority_eat_target(actions: &[NightAction]) -> Option<PlayerId> {
    majority_target(actions.iter().filter_map(|a| match a {
        NightAction::EatVote { target } => Some(*target),
        _ => None,
    }))
}

/// Maps a role to its day-phase question, if it has one. Only Gunner today
/// (see `gunner` module) — everyone else has no day action, which is the
/// default `RoleBehavior::day_action` already returns.
fn one_target_day_prompt(role: Role) -> Option<Prompt> {
    match role {
        Role::Gunner => Some(Prompt::GunnerShoot),
        Role::Spumpkin => Some(Prompt::SpumpkinDetonate),
        _ => None,
    }
}

/// Roles with a yes/no day decision instead of a target pick.
fn toggle_day_prompt(role: Role) -> Option<Prompt> {
    match role {
        Role::Mayor => Some(Prompt::MayorReveal),
        Role::Pacifist => Some(Prompt::PacifistPeace),
        Role::Troublemaker => Some(Prompt::TroublemakerTrouble),
        // Blacksmith's "spread silver" is a village-wide yes/no
        // (Werewolf.cs:5083-5092), not a target pick - see
        // `roles::blacksmith`'s doc for the earlier wrong assumption this
        // corrects.
        Role::Blacksmith => Some(Prompt::BlacksmithSilver),
        _ => None,
    }
}

/// Resolves one full day: every alive player casts a lynch vote (universal
/// — unlike night actions, this isn't gated by role at all), tallied the
/// same way the wolves' eat vote is, plus whatever role-specific day
/// actions apply (currently just Gunner's shot). Like `resolve_night`,
/// this only resolves *decisions* — see `apply_day_results` for turning
/// them into deaths.
pub async fn resolve_day(
    players: &[AlivePlayer],
    states: &mut HashMap<PlayerId, RoleState>,
    presenter: &mut dyn Presenter,
) -> (Vec<DayAction>, Option<PlayerId>) {
    let alive: Vec<PlayerId> = players.iter().map(|p| p.id).collect();
    let mut day_actions = vec![];
    // Tracks voter identity, not just the target, so Mayor's own vote can
    // be found again and doubled below (Werewolf.cs:2649-2652) — a flat
    // Vec<PlayerId> of targets alone can't answer "which one was Mayor's."
    let mut votes_by_voter: Vec<(PlayerId, PlayerId)> = vec![];
    let mut mayor_revealed: Option<PlayerId> = None;

    for player in players {
        if let Some(target) = ask_one(presenter, player.id, Prompt::LynchVote, &alive).await {
            votes_by_voter.push((player.id, target));
        }

        let behavior = behavior_for(player.role);
        let state = states.entry(player.id).or_default();

        let chosen_target = match one_target_day_prompt(player.role) {
            Some(prompt) => ask_one(presenter, player.id, prompt, &alive).await,
            None => None,
        };
        let toggle_choice = match toggle_day_prompt(player.role) {
            Some(prompt) => presenter.ask_toggle(player.id, prompt).await,
            None => false,
        };

        let ctx = DayContext {
            alive: &alive,
            self_id: player.id,
            chosen_target,
            toggle_choice,
        };
        let actions = behavior.day_action(&ctx, state);
        if player.role == Role::Mayor && actions.contains(&DayAction::Reveal) {
            mayor_revealed = Some(player.id);
        }
        day_actions.extend(actions);
    }

    let mut all_votes: Vec<PlayerId> = votes_by_voter.iter().map(|&(_, target)| target).collect();
    if let Some(mayor_id) = mayor_revealed {
        if let Some(&(_, target)) = votes_by_voter.iter().find(|&&(voter, _)| voter == mayor_id) {
            all_votes.push(target); // counts twice, once revealed
        }
    }
    let lynch_target = majority_target(all_votes.into_iter());
    (day_actions, lynch_target)
}

/// Turns resolved day decisions into actual deaths — the day-phase twin of
/// `apply_night_results`. Two things can cancel the lynch entirely, unlike
/// `apply_night_results` where only Witch's heal can cancel a death:
///
/// - Pacifist's `Pacify` action vetoes the *entire* day's lynch
///   (Werewolf.cs:913-925) — checked first, since it overrides everything.
/// - Prince's first lynch is survived, not fatal (Werewolf.cs:2745-2751:
///   `!lynched.HasUsedAbility`). This needs `lynch_target_role` (who the
///   target actually is) and `states` (to check/spend the one-time
///   immunity via `RoleState::primary_used`) — parameters
///   `apply_night_results` doesn't need, since nothing on the night side
///   has this "am I this specific role, and have I used my one save yet"
///   shape.
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

    // "Trouble overrides peace" (Werewolf.cs:971): a same-day Troublemaker
    // veto cancels a Pacifist's veto, so Trouble wins if both fire.
    let troubled = day_actions.iter().any(|a| matches!(a, DayAction::Trouble));
    let pacified = !troubled && day_actions.iter().any(|a| matches!(a, DayAction::Pacify));

    if !pacified {
        if let Some(target) = lynch_target {
            let prince_has_immunity = lynch_target_role == Some(Role::Prince)
                && !states.entry(target).or_default().primary_used;
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
            | DayAction::Detonate { .. } => {}
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
        let deaths = apply_day_results(
            &[DayAction::Pacify],
            Some(target),
            Some(Role::Villager),
            &mut states,
        );
        assert_eq!(deaths, vec![], "Pacifist's veto should cancel the lynch");
    }

    #[test]
    fn pacify_does_not_cancel_a_gunner_shot() {
        let lynch_target = PlayerId(1);
        let shot_target = PlayerId(2);
        let mut states = HashMap::new();
        let deaths = apply_day_results(
            &[DayAction::Pacify, DayAction::Shoot { shooter: PlayerId(99), target: shot_target }],
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
}

/// Turns resolved night decisions into actual deaths — the step
/// `resolve_night`'s own doc comment flagged as future work. Deliberately
/// narrow: only the two cases this codebase's role logic actually models
/// resolve to a death.
///
/// - The wolves' `wolf_target` dies of `KillMethod::Eat`, **unless**
///   Witch's `Heal` action or Guardian Angel's `Protect` action names that
///   exact same player (Werewolf.cs:3280-3286: `if (ga?.Choice ==
///   target.Id) { ... target.WasSavedLastNight = true; }` — the wolf kill
///   simply never happens, the same shape as the heal potion, which is
///   why they share one cancellation check below rather than each role
///   getting its own).
/// - Witch's `Poison` target dies of `KillMethod::Poison`, unconditionally
///   and independently of the wolf kill.
/// - The Serial Killer's `SerialKillVote` target dies of
///   `KillMethod::SerialKilled`, unconditionally — same "no consensus
///   needed" reasoning as the role file itself.
///
/// Everything else this proof-of-concept resolves as a *decision*
/// (`Visit`, `Investigate`, `CheckTeam`, `ChooseRoleModel`, `ConvertVote`)
/// has no death consequence modeled yet — Harlot dying from visiting a
/// wolf, the Guardian Angel herself risking death instead of the target
/// (Werewolf.cs:2361-2372), cult conversion's RNG resolution, are real
/// legacy mechanics (see `harlot`/`guardian_angel`/`cultist` module docs)
/// that need cross-player resolution logic this function doesn't attempt.
/// If several causes target the same player, they appear once per cause —
/// deduplicating "someone already dead" is future work for whoever
/// applies this to real running game state.
///
/// Two things can suppress the wolf kill entirely, on top of the heal/
/// protect cancellation above:
/// - `actions` containing `NightAction::SandmanSleep` (Werewolf.cs:3011-
///   3020: the whole night is skipped, `return;` before any night action
///   resolves) — checked first, and short-circuits everything else this
///   function does, not just the wolf kill.
/// - `silver_spread` being `true` (Werewolf.cs:5191: `if (!_silverSpread)
///   { ...assign wolf targets... }` — nothing runs in the `else`, so the
///   wolves simply have no target the night after a Blacksmith uses their
///   ability). Passed in rather than derived from `actions` because it's
///   cross-round state (the Blacksmith's decision was a *day* action the
///   round before) that `run_game` is responsible for carrying forward,
///   not something this single night's actions could know on their own.
pub fn apply_night_results(
    actions: &[NightAction],
    wolf_target: Option<PlayerId>,
    silver_spread: bool,
) -> Vec<(PlayerId, shared::KillMethod)> {
    let mut deaths = vec![];

    if actions.iter().any(|a| matches!(a, NightAction::SandmanSleep)) {
        return deaths;
    }

    let healed_target = actions.iter().find_map(|a| match a {
        NightAction::Heal { target } => Some(*target),
        _ => None,
    });
    let protected_target = actions.iter().find_map(|a| match a {
        NightAction::Protect { target } => Some(*target),
        _ => None,
    });

    if !silver_spread {
        if let Some(target) = wolf_target {
            if healed_target != Some(target) && protected_target != Some(target) {
                deaths.push((target, shared::KillMethod::Eat));
            }
        }
    }

    for action in actions {
        match action {
            NightAction::Poison { target } => {
                deaths.push((*target, shared::KillMethod::Poison));
            }
            NightAction::SerialKillVote { target } => {
                deaths.push((*target, shared::KillMethod::SerialKilled));
            }
            _ => {}
        }
    }

    deaths
}

/// The "a death can trigger one more question" case `apply_night_results`/
/// `apply_day_results` can't express by themselves — they only ever turn
/// already-made decisions into deaths, never ask a new one afterward
/// (Werewolf.cs:5420-5484: `HunterFinalShot`, called the moment a Hunter
/// dies, by *any* method). Called by `run_game` right after a batch of
/// deaths is applied, with the roster still reflecting who survived that
/// batch: every dead Hunter in `died_with_roles` gets asked one
/// `ask_targets` question — a different `Prompt` depending on whether they
/// were lynched or killed some other way, since the legacy game's own
/// text differs by exactly that (`killMethod == KillMthd.Lynch`) — and
/// whoever they pick dies too, via `KillMethod::HunterShot`.
///
/// **Scoped to the core mechanic only.** The legacy code passes
/// `hunterFinalShot: false` (skipping this entirely) for several specific
/// cases this proof-of-concept doesn't distinguish: dying by falling into
/// a Grave Digger's trap, an idle/flee death, an Arsonist's mass burn, a
/// wolf-standoff sub-case where the Hunter already shot back at a visiting
/// wolf, and a couple of hardcoded 2-player endgame shortcuts. Every one
/// of those would incorrectly grant a shot here.
///
/// One of those cases *is* modeled precisely, though: a plain wolf-eat
/// (`KillMethod::Eat`) death always passes `hunterFinalShot: false`
/// (Werewolf.cs:3419, the `default:` case of the wolf-eat resolution
/// switch — reached by every eaten role including Hunter itself, whose
/// own case falls through to it when its shoot-back RNG fails,
/// Werewolf.cs:3413) — confirmed to apply uniformly to both the primary
/// wolf-eat and the Wolf Cub's bonus kill, since both targets are pushed
/// through this same switch (Werewolf.cs:3239-3270). So `KillMethod::Eat`
/// is excluded here entirely, unconditionally — no question is asked at
/// all for that death method, not just a different prompt.
///
/// Also doesn't model "Domino" chaining (a Hunter's shot landing on
/// *another* Hunter, who would get their own final shot in turn,
/// Werewolf.cs:5480-5481) — this only asks each already-dead Hunter from
/// the triggering batch once, not recursively.
pub async fn resolve_hunter_shots(
    new_deaths: &[(PlayerId, KillMethod)],
    died_with_roles: &[(PlayerId, Role)],
    alive: &[AlivePlayer],
    presenter: &mut dyn Presenter,
) -> Vec<(PlayerId, KillMethod)> {
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
        if method == KillMethod::Eat {
            continue;
        }
        let prompt = if method == KillMethod::Lynch {
            Prompt::HunterFinalShotLynched
        } else {
            Prompt::HunterFinalShotKilled
        };
        let options: Vec<PlayerId> = alive_ids.iter().copied().filter(|&id| id != hunter_id).collect();
        if let Some(target) = ask_one(presenter, hunter_id, prompt, &options).await {
            shots.push((target, KillMethod::HunterShot));
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
        let already_dying = new_deaths.iter().any(|&(v, _)| v == partner)
            || chain.iter().any(|&(v, _)| v == partner);
        if partner_alive && !already_dying {
            chain.push((partner, KillMethod::LoverDied));
        }
    }

    chain
}

/// Three of `VisitPlayer`'s outcomes for a Harlot, all unconditional (no
/// RNG involved, unlike most of that function's other branches — the one
/// coin flip it does roll, Werewolf.cs:2340: `Program.R.Next(100) < 80`,
/// only applies to a *wolf-team* visitor, gated behind
/// `!WolfRoles.Contains(visitor.PlayerRole)`, which is always true for a
/// Harlot):
///
/// - Visiting whoever the wolves actually ate that same night kills her
///   too (Werewolf.cs:2352-2360: `KillMthd.VisitWolf`).
/// - Visiting the Serial Killer *player themself* kills her
///   (Werewolf.cs:2339-2345: `visited.PlayerRole == IRole.SerialKiller`
///   unconditionally reaches `KillMthd.VisitKiller` for a non-wolf
///   visitor).
/// - Visiting whoever the Serial Killer separately killed that same
///   night (a *different* player from the SK themself) also kills her
///   (Werewolf.cs:3893-3897: the `VisitResult.AlreadyDead` case,
///   `KillMthd.VisitVictim`, gated on `target.KilledByRole ==
///   IRole.SerialKiller`).
///
/// Takes `night_actions`/`alive` rather than just the two targets because
/// it needs to find *which* Harlot visited, if any (`NightAction::Visit`
/// carries her own id), and needs the visited player's *role* to tell
/// "visited the Serial Killer" apart from "visited the Serial Killer's
/// victim" — two different keys for two different targets that happen to
/// share a `killedByRole`.
pub fn resolve_harlot_visit_deaths(
    night_actions: &[NightAction],
    wolf_target: Option<PlayerId>,
    alive: &[AlivePlayer],
) -> Vec<(PlayerId, KillMethod)> {
    let sk_target = night_actions.iter().find_map(|a| match a {
        NightAction::SerialKillVote { target } => Some(*target),
        _ => None,
    });

    night_actions
        .iter()
        .filter_map(|a| match a {
            NightAction::Visit { visitor, target } if Some(*target) == wolf_target => {
                Some((*visitor, KillMethod::VisitWolf))
            }
            NightAction::Visit { visitor, target }
                if alive.iter().any(|p| p.id == *target && p.role == Role::SerialKiller) =>
            {
                Some((*visitor, KillMethod::VisitKiller))
            }
            NightAction::Visit { visitor, target } if Some(*target) == sk_target => {
                Some((*visitor, KillMethod::VisitVictim))
            }
            _ => None,
        })
        .collect()
}

/// Arsonist's mass burn (Werewolf.cs:3190-3202): choosing to detonate
/// (`NightAction::Detonate`, "Spark") kills every currently-doused,
/// still-alive player at once (`KillMethod::Burn`) — except whoever a
/// Guardian Angel is protecting this exact night, who's saved instead
/// (Werewolf.cs:3193-3195: `ga?.Choice == burn.Id` skips the `KillPlayer`
/// call entirely for that one player, no partial protection for anyone
/// else in the batch). Unconditional otherwise, no RNG.
///
/// Doused status accumulates across nights — `game::run_game` tracks the
/// full set itself rather than through `RoleState`, since
/// `arsonist::Arsonist`'s own doc flags that `RoleState::remembered_player`
/// is a single slot, not the `Vec` this needs. Returns `None` (rather
/// than an empty `Vec`) when no detonation happened this night, so a
/// caller can tell "sparked with nobody doused" apart from "didn't spark
/// at all" — only the latter should leave the doused set untouched for
/// next night.
pub fn resolve_arsonist_detonation(
    night_actions: &[NightAction],
    doused: &[PlayerId],
    alive: &[AlivePlayer],
) -> Option<Vec<(PlayerId, KillMethod)>> {
    if !night_actions.iter().any(|a| matches!(a, NightAction::Detonate)) {
        return None;
    }
    let protected_target = night_actions.iter().find_map(|a| match a {
        NightAction::Protect { target } => Some(*target),
        _ => None,
    });
    Some(
        doused
            .iter()
            .filter(|&&p| Some(p) != protected_target && alive.iter().any(|a| a.id == p))
            .map(|&p| (p, KillMethod::Burn))
            .collect(),
    )
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
fn grave_digger_fall_chance_percent(dug_graves: u32, visitor_is_village: bool) -> u32 {
    let chance = 20.0 + (30.0 - 30.0 * 0.5_f64.powi(dug_graves as i32 - 1));
    let chance = if visitor_is_village { chance / 2.0 } else { chance };
    chance.round() as u32
}

/// Whoever visits a Grave Digger who's dug at least one grave risks
/// falling in and dying instead of whatever their visit would normally
/// do (Werewolf.cs:2377-2411, `KillMthd.FallGrave`, always
/// `hunterFinalShot: false`). Scoped to the two visitor roles this
/// proof-of-concept can name a single, definite visitor for:
///
/// - Harlot (`NightAction::Visit`) — falls into `HarlotFellPublic`.
/// - Guardian Angel (`NightAction::Protect`) — falls into
///   `DefaultFellPublic` (not one of the switch's named exceptions,
///   Werewolf.cs:4297-4300).
///
/// **Deliberately not modeled**: a Serial Killer visiting a Grave Digger
/// never falls at all (Werewolf.cs:2382-2387, `StumbledGrave` is pure
/// flavor tracking — the visit still resolves as a normal success,
/// killing the Grave Digger like any other `SerialKillVote` target), so
/// there's no SK case to add here — this isn't a gap, it's the real rule.
/// Wolves visiting a Grave Digger *can* fall (Werewolf.cs's `default`
/// case, `WolfFellPublic`), but the real code picks one specific wolf out
/// of the voting pack to be "the visitor" (`voteWolves.ElementAt(...)`,
/// see `RngSource::pick_index`) — this proof-of-concept's wolf-eat vote
/// only tracks a majority *target*, not which individual wolf cast the
/// deciding visit, so there's no single identity to attribute the fall
/// to yet. Left out rather than attributed to an arbitrary wolf.
///
/// The Guardian Angel case also skips one real exception
/// (Werewolf.cs:2385: a GA who already saved the Grave Digger some other
/// way this exact night is safe) — this proof-of-concept doesn't track
/// `WasSavedLastNight` outside the wolf-protect check, so it isn't
/// checked here either; an honest simplification, not an oversight.
pub fn resolve_grave_digger_falls(
    night_actions: &[NightAction],
    dug_graves: u32,
    alive: &[AlivePlayer],
    rng: &mut dyn RngSource,
) -> Vec<(PlayerId, KillMethod)> {
    if dug_graves < 1 {
        return vec![];
    }
    let chance = grave_digger_fall_chance_percent(dug_graves, true); // both candidate visitors are Village-team

    let is_grave_digger = |id: PlayerId| alive.iter().any(|p| p.id == id && p.role == Role::GraveDigger);

    let mut falls = vec![];
    for action in night_actions {
        let visitor = match action {
            NightAction::Visit { visitor, target } if is_grave_digger(*target) => Some(*visitor),
            // `Protect` doesn't carry the protector's own id the way
            // `Visit` does (see `NightAction::Protect`'s doc) - look up
            // whoever's alive as a Guardian Angel instead, the same "there's
            // normally only one" reasoning `resolve_wolf_cub_bonus_kill` and
            // friends already rely on elsewhere in this file.
            NightAction::Protect { target } if is_grave_digger(*target) => {
                alive.iter().find(|p| p.role == Role::GuardianAngel).map(|p| p.id)
            }
            _ => None,
        };
        if let Some(visitor) = visitor {
            if rng.chance(chance) {
                falls.push((visitor, KillMethod::FallGrave));
            }
        }
    }
    falls
}

/// Chemist's success chance (Werewolf.cs `Settings.cs:105,131`:
/// `ChemistSuccessChance = 50`) — a per-group setting in the real game,
/// same "no group settings model, always the real default" reasoning as
/// `ALPHA_WOLF_CONVERSION_CHANCE_PERCENT` in `game`.
const CHEMIST_SUCCESS_CHANCE_PERCENT: u32 = 50;

/// The Chemist's nightly gambit (Werewolf.cs:3792-3821): a coin flip on
/// each `NightAction::Chemistry` — success kills the target
/// (`KillMthd.Chemistry`), failure kills the Chemist instead (the same
/// method; the real code passes the Chemist as both victim and killer for
/// the failure case too, `KillPlayer(chemist, KillMthd.Chemistry, killer:
/// chemist)`).
///
/// Simplified relative to the source in one way, honestly: the real
/// gambit first runs the target through the generic `VisitPlayer`
/// (blocked by a wolf/Serial Killer/Grave Digger/burning target, or a
/// Harlot/Guardian Angel who stepped out), and only rolls the success
/// chance if that visit itself succeeds. This proof-of-concept has no
/// generic "was this visit even let through" resolution for an arbitrary
/// target the way `resolve_grave_digger_falls`/`resolve_harlot_visit_deaths`
/// narrowly do for their own specific visitor roles — so a Chemist
/// targeting, say, a wolf or another Grave Digger is treated as an
/// ordinary successful visit here, which the real game would have
/// resolved differently. An honest gap, not a silent one.
pub fn resolve_chemistry(
    night_actions: &[NightAction],
    rng: &mut dyn RngSource,
) -> Vec<(PlayerId, KillMethod)> {
    night_actions
        .iter()
        .filter_map(|a| match a {
            NightAction::Chemistry { chemist, target } => {
                let victim = if rng.chance(CHEMIST_SUCCESS_CHANCE_PERCENT) { *target } else { *chemist };
                Some((victim, KillMethod::Chemistry))
            }
            _ => None,
        })
        .collect()
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
/// and whether a Hunter final shot could trigger — irrelevant here (this
/// proof-of-concept doesn't write history rows, and a Spumpkin isn't a
/// Hunter), so both deaths are given the same `KillMethod::Shoot` rather
/// than inventing a "no method" case this crate's `(PlayerId, KillMethod)`
/// death shape has no room for.
///
/// The WiseElder-demotion special case (Werewolf.cs:2916: detonating a
/// Wise Elder transforms the *Spumpkin* to Villager first) doesn't change
/// who dies — `KillPlayer(spumpkin, ...)` and `KillPlayer(check, ...)`
/// both still run unconditionally right after, regardless of which
/// branch of that `switch` fired — so it's flavor/achievement bookkeeping
/// only, safe to skip without changing lethality.
pub fn resolve_spumpkin_detonation(
    day_actions: &[DayAction],
    rng: &mut dyn RngSource,
) -> Vec<(PlayerId, KillMethod)> {
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

/// What a resolved cult-conversion attempt did — a converted target
/// (`transform`), a chained death (`deaths` — the cult's "newbie" member,
/// the one who does the actual visiting, dying instead of converting
/// anyone), or neither, never both (matches the real
/// `ConvertToCult`/`VisitPlayer` interaction: a conversion attempt either
/// succeeds, fails safely, or gets the visitor killed — it's never a
/// death *and* a conversion the same night).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CultConversionOutcome {
    pub transform: Option<PlayerId>,
    pub deaths: Vec<(PlayerId, KillMethod)>,
}

/// Every named `*ConversionChance` in Werewolf.cs's `Settings.cs`
/// (both settings profiles agree on every value). Any role *not* listed
/// here converts at the real default of 100% (Werewolf.cs:2489:
/// `ConvertToCult(IPlayer target, ..., int chance = 100)`) — a
/// Villager/Mason/Prince/Mayor/Gunner/etc. visited by the cult always
/// converts, no roll needed, matching the C# default parameter rather
/// than this proof-of-concept inventing its own "usually converts"
/// number. `Doppelganger`/`Thief`/`Spumpkin`/`Arsonist` are the one
/// group that goes the *other* way — Werewolf.cs:3729-3745 calls
/// `ConvertToCult(target, voteCult, 0)` for all four (guaranteed to
/// fail), so they're listed explicitly at `0` rather than silently
/// falling into the same "no chance value found" bucket a typo would
/// produce.
fn cult_conversion_chance_percent(role: Role) -> u32 {
    match role {
        Role::Oracle => 50,
        Role::Sandman => 60,
        Role::WiseElder => 30,
        Role::Pacifist => 80,
        Role::Augur => 40,
        Role::Seer => 40,
        Role::GuardianAngel => 60,
        Role::Detective => 70,
        Role::Cursed => 60,
        Role::Harlot => 70,
        Role::Sorcerer => 40,
        Role::Blacksmith => 75,
        Role::Doppelganger | Role::Thief | Role::Spumpkin | Role::Arsonist => 0,
        _ => 100,
    }
}

/// The cult's nightly recruitment attempt (Werewolf.cs:3591-3757):
/// majority-tallies `NightAction::ConvertVote` the same way the wolf-eat
/// vote is tallied, then routes the target through several special cases
/// before falling back to `cult_conversion_chance_percent`'s per-role
/// roll. `newbie` — the cult's most-recently-converted member, who does
/// the actual visiting and is the one who dies if a visit goes wrong
/// (`voteCult.OrderByDescending(x => x.DayCult).First()`) — is
/// approximated as *any* alive Cultist here: this proof-of-concept
/// doesn't track conversion order (`DayCult`), so there's no real
/// "newest" to pick: an honest simplification, not a bug.
///
/// Special cases, in the order the source checks them:
/// - **Grave Digger** target: subject to the same fall trap
///   `resolve_grave_digger_falls` models for Harlot/Guardian Angel
///   (Werewolf.cs's shared `VisitPlayer`, reached before any of the
///   per-role branches below) — except a Cultist isn't Village-team, so
///   the halving those two visitors get doesn't apply here
///   (`grave_digger_fall_chance_percent(dug_graves, false)`).
/// - **Hunter** target: a nested pair of rolls (`HunterConversionChance`
///   then, only if that fails, `HunterKillCultChance`) — convert, kill
///   the newbie, or neither.
/// - **CultistHunter** target: kills the newbie unconditionally
///   (Werewolf.cs:3654-3656, `KillMthd.Hunt`) — no roll at all.
/// - **Wolf-team** target (`Wolf`/`AlphaWolf`/`WolfCub`/`Lycan`): the
///   newbie only dies if the wolves stayed home that night
///   (Werewolf.cs:3669: `voteWolves.Any(... Choice ...)` — "did the
///   wolves go eating"), approximated here as `wolves_went_eating`
///   (whether the main wolf-eat vote had a target at all, the same
///   signal `resolve_wolf_eat_target`'s caller already has as
///   `wolf_target.is_some()`).
/// - **Snow Wolf** target: the real check is whether *that specific
///   player* went out freezing someone (Werewolf.cs:3687:
///   `target.Choice != -1 && != 0`), not the main wolf vote — but
///   `snow_wolf::SnowWolf`'s freeze ability isn't modeled at all (see
///   that file's doc), so a Snow Wolf here is always "stayed home,"
///   always killing the newbie. Consistent with the rest of this crate
///   treating an unmodeled ability as simply never exercised.
/// - Everything else: `cult_conversion_chance_percent(target_role)`.
pub fn resolve_cultist_conversion(
    night_actions: &[NightAction],
    wolves_went_eating: bool,
    dug_graves: u32,
    alive: &[AlivePlayer],
    rng: &mut dyn RngSource,
) -> CultConversionOutcome {
    let no_outcome = CultConversionOutcome::default();

    let Some(target) = majority_target(night_actions.iter().filter_map(|a| match a {
        NightAction::ConvertVote { target } => Some(*target),
        _ => None,
    })) else {
        return no_outcome;
    };
    let Some(newbie) = alive.iter().find(|p| p.role == Role::Cultist).map(|p| p.id) else {
        return no_outcome;
    };
    let Some(target_role) = alive.iter().find(|p| p.id == target).map(|p| p.role) else {
        return no_outcome;
    };

    if target_role == Role::GraveDigger {
        return if dug_graves >= 1 && rng.chance(grave_digger_fall_chance_percent(dug_graves, false)) {
            CultConversionOutcome { transform: None, deaths: vec![(newbie, KillMethod::FallGrave)] }
        } else {
            no_outcome
        };
    }

    const HUNTER_CONVERSION_CHANCE_PERCENT: u32 = 50;
    const HUNTER_KILL_CULT_CHANCE_PERCENT: u32 = 50;

    match target_role {
        Role::Hunter => {
            if rng.chance(HUNTER_CONVERSION_CHANCE_PERCENT) {
                CultConversionOutcome { transform: Some(target), deaths: vec![] }
            } else if rng.chance(HUNTER_KILL_CULT_CHANCE_PERCENT) {
                CultConversionOutcome { transform: None, deaths: vec![(newbie, KillMethod::HunterCult)] }
            } else {
                no_outcome
            }
        }
        Role::CultistHunter => CultConversionOutcome { transform: None, deaths: vec![(newbie, KillMethod::Hunt)] },
        Role::Wolf | Role::AlphaWolf | Role::WolfCub | Role::Lycan => {
            if wolves_went_eating {
                no_outcome
            } else {
                CultConversionOutcome { transform: None, deaths: vec![(newbie, KillMethod::VisitWolf)] }
            }
        }
        Role::SnowWolf => CultConversionOutcome { transform: None, deaths: vec![(newbie, KillMethod::VisitWolf)] },
        _ => {
            if rng.chance(cult_conversion_chance_percent(target_role)) {
                CultConversionOutcome { transform: Some(target), deaths: vec![] }
            } else {
                no_outcome
            }
        }
    }
}

/// The wolves' bonus kill after a Wolf Cub dies (Werewolf.cs:1047-1053,
/// `WolfCubKilled`): every surviving wolf-team member gets asked a second
/// "who do you want to eat?" question (`Prompt::WolfCubBonusKill`, which
/// reuses the real `AskEat` text — the legacy code's own comment does
/// exactly the same reuse, `GetLocaleString("AskEat")`), majority-tallied
/// the same way the main wolf vote is, excluding wolf-team targets same
/// as the original.
///
/// Called by `run_game` at the *start* of the next night, alongside (not
/// after) the main wolf vote — confirmed by reading `NightCycle`
/// directly: `SendNightActions` (which sends that night's menus,
/// Werewolf.cs:3030) runs before `WolfCubKilled` gets reset
/// (Werewolf.cs:3235), so a wolf answering the main "who do you eat?"
/// question is immediately chained into this second question the same
/// night, off whatever's left over from the last death, and both
/// answers resolve together at the end of that night. Asking this
/// *after* the main kill has already resolved — an earlier version of
/// this call site did exactly that — was the wrong sequencing.
pub async fn resolve_wolf_cub_bonus_kill(
    alive: &[AlivePlayer],
    presenter: &mut dyn Presenter,
) -> Option<PlayerId> {
    let non_wolf_ids: Vec<PlayerId> = alive
        .iter()
        .filter(|p| !crate::is_wolf_muscle(p.role))
        .map(|p| p.id)
        .collect();

    let mut votes = vec![];
    for player in alive.iter().filter(|p| crate::is_wolf_muscle(p.role)) {
        if let Some(target) = ask_one(presenter, player.id, Prompt::WolfCubBonusKill, &non_wolf_ids).await {
            votes.push(target);
        }
    }

    majority_target(votes.into_iter())
}

#[cfg(test)]
mod apply_night_results_tests {
    use super::*;
    use shared::KillMethod;

    #[test]
    fn wolf_target_dies_when_nobody_heals() {
        let target = PlayerId(1);
        let deaths = apply_night_results(&[], Some(target), false);
        assert_eq!(deaths, vec![(target, KillMethod::Eat)]);
    }

    #[test]
    fn heal_on_the_wolf_target_cancels_the_kill() {
        let target = PlayerId(1);
        let actions = [NightAction::Heal { target }];
        let deaths = apply_night_results(&actions, Some(target), false);
        assert_eq!(deaths, vec![], "healing the wolf's own target should cancel it");
    }

    #[test]
    fn protecting_the_wolf_target_also_cancels_the_kill() {
        let target = PlayerId(1);
        let actions = [NightAction::Protect { target }];
        let deaths = apply_night_results(&actions, Some(target), false);
        assert_eq!(
            deaths,
            vec![],
            "Guardian Angel protecting the wolves' actual target should cancel it, same as Witch's heal"
        );
    }

    #[test]
    fn protecting_someone_else_does_not_cancel_the_wolf_kill() {
        let target = PlayerId(1);
        let actions = [NightAction::Protect {
            target: PlayerId(2),
        }];
        let deaths = apply_night_results(&actions, Some(target), false);
        assert_eq!(deaths, vec![(target, KillMethod::Eat)]);
    }

    #[test]
    fn heal_on_someone_else_does_not_cancel_the_wolf_kill() {
        let target = PlayerId(1);
        let actions = [NightAction::Heal {
            target: PlayerId(2),
        }];
        let deaths = apply_night_results(&actions, Some(target), false);
        assert_eq!(deaths, vec![(target, KillMethod::Eat)]);
    }

    #[test]
    fn poison_kills_independently_of_the_wolf_target() {
        let wolf_target = PlayerId(1);
        let poisoned = PlayerId(2);
        let actions = [NightAction::Poison { target: poisoned }];
        let mut deaths = apply_night_results(&actions, Some(wolf_target), false);
        deaths.sort_by_key(|(id, _)| id.0);
        assert_eq!(
            deaths,
            vec![
                (wolf_target, KillMethod::Eat),
                (poisoned, KillMethod::Poison)
            ]
        );
    }

    #[test]
    fn no_wolf_target_means_no_eat_death() {
        assert_eq!(apply_night_results(&[], None, false), vec![]);
    }

    #[test]
    fn serial_kill_vote_kills_unconditionally() {
        let target = PlayerId(5);
        let actions = [NightAction::SerialKillVote { target }];
        assert_eq!(
            apply_night_results(&actions, None, false),
            vec![(target, KillMethod::SerialKilled)]
        );
    }

    #[test]
    fn sandman_sleep_cancels_every_death_this_night() {
        let wolf_target = PlayerId(1);
        let poisoned = PlayerId(2);
        let actions = [
            NightAction::SandmanSleep,
            NightAction::Poison { target: poisoned },
        ];
        assert_eq!(
            apply_night_results(&actions, Some(wolf_target), false),
            vec![],
            "a Sandman sleep should cancel the wolf kill AND anything else that night"
        );
    }

    #[test]
    fn silver_spread_cancels_only_the_wolf_kill() {
        let wolf_target = PlayerId(1);
        let poisoned = PlayerId(2);
        let actions = [NightAction::Poison { target: poisoned }];
        let deaths = apply_night_results(&actions, Some(wolf_target), true);
        assert_eq!(
            deaths,
            vec![(poisoned, KillMethod::Poison)],
            "silver should block the wolf kill but not an unrelated Poison"
        );
    }

    #[test]
    fn without_silver_spread_the_wolf_kill_proceeds_normally() {
        let target = PlayerId(1);
        assert_eq!(
            apply_night_results(&[], Some(target), false),
            vec![(target, KillMethod::Eat)]
        );
    }
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

    fn alive_roster() -> Vec<AlivePlayer> {
        vec![
            AlivePlayer { id: PlayerId(2), role: Role::Villager },
            AlivePlayer { id: PlayerId(3), role: Role::Seer },
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

        let shots = resolve_hunter_shots(&new_deaths, &died_with_roles, &alive_roster(), &mut presenter).await;

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

        resolve_hunter_shots(&new_deaths, &died_with_roles, &alive_roster(), &mut presenter).await;

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

        let shots = resolve_hunter_shots(&new_deaths, &died_with_roles, &alive_roster(), &mut presenter).await;

        assert_eq!(shots, vec![]);
        assert!(presenter.asked.is_empty(), "a wolf-eaten hunter should never be asked for a final shot");
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

        let shots = resolve_hunter_shots(&new_deaths, &died_with_roles, &alive_roster(), &mut presenter).await;

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

        let shots = resolve_hunter_shots(&new_deaths, &died_with_roles, &alive_roster(), &mut presenter).await;

        assert_eq!(shots, vec![]);
        assert!(presenter.asked.is_empty(), "a non-Hunter death should never trigger a final-shot question");
    }
}

#[cfg(test)]
mod lover_death_tests {
    use super::*;

    fn alive_pair() -> Vec<AlivePlayer> {
        vec![
            AlivePlayer { id: PlayerId(2), role: Role::Villager },
            AlivePlayer { id: PlayerId(3), role: Role::Seer },
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
mod cultist_conversion_tests {
    use super::*;
    use crate::rng::ScriptedRngSource;

    fn roster() -> Vec<AlivePlayer> {
        vec![
            AlivePlayer { id: PlayerId(1), role: Role::Cultist },
            AlivePlayer { id: PlayerId(2), role: Role::Seer },
            AlivePlayer { id: PlayerId(3), role: Role::Hunter },
            AlivePlayer { id: PlayerId(4), role: Role::CultistHunter },
            AlivePlayer { id: PlayerId(5), role: Role::Wolf },
            AlivePlayer { id: PlayerId(6), role: Role::SnowWolf },
            AlivePlayer { id: PlayerId(7), role: Role::GraveDigger },
            AlivePlayer { id: PlayerId(8), role: Role::Villager },
        ]
    }

    fn vote(target: PlayerId) -> Vec<NightAction> {
        vec![NightAction::ConvertVote { target }]
    }

    #[test]
    fn no_votes_means_no_outcome_at_all() {
        let mut rng = ScriptedRngSource::new(); // panics if consulted
        let outcome = resolve_cultist_conversion(&[], true, 0, &roster(), &mut rng);
        assert_eq!(outcome, CultConversionOutcome::default());
    }

    #[test]
    fn a_default_role_always_converts() {
        let mut rng = ScriptedRngSource::new().with_chance(true);
        let outcome = resolve_cultist_conversion(&vote(PlayerId(8)), true, 0, &roster(), &mut rng);
        assert_eq!(outcome.transform, Some(PlayerId(8)));
        assert_eq!(outcome.deaths, vec![]);
    }

    #[test]
    fn a_seer_only_converts_on_a_successful_roll() {
        let mut rng = ScriptedRngSource::new().with_chance(false);
        let outcome = resolve_cultist_conversion(&vote(PlayerId(2)), true, 0, &roster(), &mut rng);
        assert_eq!(outcome, CultConversionOutcome::default());
    }

    #[test]
    fn a_hunter_who_converts_never_rolls_the_kill_chance() {
        let mut rng = ScriptedRngSource::new().with_chance(true);
        let outcome = resolve_cultist_conversion(&vote(PlayerId(3)), true, 0, &roster(), &mut rng);
        assert_eq!(outcome.transform, Some(PlayerId(3)));
    }

    #[test]
    fn a_hunter_who_fails_to_convert_may_kill_the_newbie() {
        let mut rng = ScriptedRngSource::new().with_chance(false).with_chance(true);
        let outcome = resolve_cultist_conversion(&vote(PlayerId(3)), true, 0, &roster(), &mut rng);
        assert_eq!(outcome, CultConversionOutcome { transform: None, deaths: vec![(PlayerId(1), KillMethod::HunterCult)] });
    }

    #[test]
    fn a_hunter_who_fails_both_rolls_does_nothing() {
        let mut rng = ScriptedRngSource::new().with_chance(false).with_chance(false);
        let outcome = resolve_cultist_conversion(&vote(PlayerId(3)), true, 0, &roster(), &mut rng);
        assert_eq!(outcome, CultConversionOutcome::default());
    }

    #[test]
    fn a_cultist_hunter_always_kills_the_newbie_no_roll() {
        let mut rng = ScriptedRngSource::new(); // panics if consulted
        let outcome = resolve_cultist_conversion(&vote(PlayerId(4)), true, 0, &roster(), &mut rng);
        assert_eq!(outcome, CultConversionOutcome { transform: None, deaths: vec![(PlayerId(1), KillMethod::Hunt)] });
    }

    #[test]
    fn visiting_a_wolf_who_went_eating_is_a_safe_empty_visit() {
        let mut rng = ScriptedRngSource::new(); // panics if consulted
        let outcome = resolve_cultist_conversion(&vote(PlayerId(5)), true, 0, &roster(), &mut rng);
        assert_eq!(outcome, CultConversionOutcome::default());
    }

    #[test]
    fn visiting_a_wolf_who_stayed_home_kills_the_newbie() {
        let mut rng = ScriptedRngSource::new(); // panics if consulted
        let outcome = resolve_cultist_conversion(&vote(PlayerId(5)), false, 0, &roster(), &mut rng);
        assert_eq!(outcome, CultConversionOutcome { transform: None, deaths: vec![(PlayerId(1), KillMethod::VisitWolf)] });
    }

    #[test]
    fn visiting_a_snow_wolf_always_kills_the_newbie_since_freeze_isnt_modeled() {
        let mut rng = ScriptedRngSource::new(); // panics if consulted
        // even with wolves_went_eating true, SnowWolf isn't part of that vote
        let outcome = resolve_cultist_conversion(&vote(PlayerId(6)), true, 0, &roster(), &mut rng);
        assert_eq!(outcome, CultConversionOutcome { transform: None, deaths: vec![(PlayerId(1), KillMethod::VisitWolf)] });
    }

    #[test]
    fn visiting_a_grave_digger_with_no_dug_graves_is_always_safe() {
        let mut rng = ScriptedRngSource::new(); // panics if consulted
        let outcome = resolve_cultist_conversion(&vote(PlayerId(7)), true, 0, &roster(), &mut rng);
        assert_eq!(outcome, CultConversionOutcome::default());
    }

    #[test]
    fn visiting_a_grave_digger_can_catch_the_newbie_in_the_trap() {
        let mut rng = ScriptedRngSource::new().with_chance(true);
        let outcome = resolve_cultist_conversion(&vote(PlayerId(7)), true, 1, &roster(), &mut rng);
        assert_eq!(outcome, CultConversionOutcome { transform: None, deaths: vec![(PlayerId(1), KillMethod::FallGrave)] });
    }

    #[test]
    fn no_cultists_alive_means_no_outcome() {
        let mut rng = ScriptedRngSource::new(); // panics if consulted
        let roster_without_cultist: Vec<AlivePlayer> =
            roster().into_iter().filter(|p| p.role != Role::Cultist).collect();
        let outcome = resolve_cultist_conversion(&vote(PlayerId(8)), true, 0, &roster_without_cultist, &mut rng);
        assert_eq!(outcome, CultConversionOutcome::default());
    }
}

#[cfg(test)]
mod chemistry_tests {
    use super::*;
    use crate::rng::ScriptedRngSource;

    #[test]
    fn a_successful_gambit_kills_the_target() {
        let chemist = PlayerId(1);
        let target = PlayerId(2);
        let actions = [NightAction::Chemistry { chemist, target }];
        let mut rng = ScriptedRngSource::new().with_chance(true);

        assert_eq!(resolve_chemistry(&actions, &mut rng), vec![(target, KillMethod::Chemistry)]);
    }

    #[test]
    fn a_failed_gambit_kills_the_chemist_instead() {
        let chemist = PlayerId(1);
        let target = PlayerId(2);
        let actions = [NightAction::Chemistry { chemist, target }];
        let mut rng = ScriptedRngSource::new().with_chance(false);

        assert_eq!(resolve_chemistry(&actions, &mut rng), vec![(chemist, KillMethod::Chemistry)]);
    }

    #[test]
    fn no_chemistry_action_means_no_roll_at_all() {
        let mut rng = ScriptedRngSource::new(); // panics if consulted
        assert_eq!(resolve_chemistry(&[], &mut rng), vec![]);
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
    use crate::rng::ScriptedRngSource;

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

    fn roster_with_grave_digger() -> Vec<AlivePlayer> {
        vec![
            AlivePlayer { id: PlayerId(1), role: Role::Harlot },
            AlivePlayer { id: PlayerId(2), role: Role::GraveDigger },
            AlivePlayer { id: PlayerId(3), role: Role::GuardianAngel },
        ]
    }

    #[test]
    fn no_dug_graves_means_no_roll_at_all() {
        let harlot = PlayerId(1);
        let grave_digger = PlayerId(2);
        let actions = [NightAction::Visit { visitor: harlot, target: grave_digger }];
        // An unscripted ScriptedRngSource would panic if `chance()` were
        // called at all - proving the "dug_graves < 1" short-circuit.
        let mut rng = ScriptedRngSource::new();

        let falls = resolve_grave_digger_falls(&actions, 0, &roster_with_grave_digger(), &mut rng);

        assert_eq!(falls, vec![]);
    }

    #[test]
    fn a_harlot_who_fails_the_roll_visits_safely() {
        let harlot = PlayerId(1);
        let grave_digger = PlayerId(2);
        let actions = [NightAction::Visit { visitor: harlot, target: grave_digger }];
        let mut rng = ScriptedRngSource::new().with_chance(false);

        let falls = resolve_grave_digger_falls(&actions, 1, &roster_with_grave_digger(), &mut rng);

        assert_eq!(falls, vec![]);
    }

    #[test]
    fn a_harlot_who_fails_the_trap_falls_in_and_dies() {
        let harlot = PlayerId(1);
        let grave_digger = PlayerId(2);
        let actions = [NightAction::Visit { visitor: harlot, target: grave_digger }];
        let mut rng = ScriptedRngSource::new().with_chance(true);

        let falls = resolve_grave_digger_falls(&actions, 1, &roster_with_grave_digger(), &mut rng);

        assert_eq!(falls, vec![(harlot, KillMethod::FallGrave)]);
    }

    #[test]
    fn a_guardian_angel_protecting_the_grave_digger_can_also_fall() {
        let ga = PlayerId(3);
        let grave_digger = PlayerId(2);
        let actions = [NightAction::Protect { target: grave_digger }];
        let mut rng = ScriptedRngSource::new().with_chance(true);

        let falls = resolve_grave_digger_falls(&actions, 1, &roster_with_grave_digger(), &mut rng);

        assert_eq!(falls, vec![(ga, KillMethod::FallGrave)]);
    }

    #[test]
    fn visiting_someone_who_isnt_the_grave_digger_never_rolls() {
        let harlot = PlayerId(1);
        let ga = PlayerId(3);
        let actions = [NightAction::Visit { visitor: harlot, target: ga }];
        let mut rng = ScriptedRngSource::new(); // panics if consulted

        let falls = resolve_grave_digger_falls(&actions, 3, &roster_with_grave_digger(), &mut rng);

        assert_eq!(falls, vec![]);
    }
}

#[cfg(test)]
mod arsonist_detonation_tests {
    use super::*;

    fn roster() -> Vec<AlivePlayer> {
        vec![
            AlivePlayer { id: PlayerId(1), role: Role::Arsonist },
            AlivePlayer { id: PlayerId(2), role: Role::Villager },
            AlivePlayer { id: PlayerId(3), role: Role::Seer },
        ]
    }

    #[test]
    fn detonating_burns_every_doused_player() {
        let doused = [PlayerId(2), PlayerId(3)];
        let actions = [NightAction::Detonate];
        let deaths = resolve_arsonist_detonation(&actions, &doused, &roster());
        assert_eq!(
            deaths,
            Some(vec![(PlayerId(2), KillMethod::Burn), (PlayerId(3), KillMethod::Burn)])
        );
    }

    #[test]
    fn not_detonating_is_none_not_an_empty_batch() {
        let doused = [PlayerId(2)];
        let actions = [NightAction::Douse { target: PlayerId(2) }];
        assert_eq!(resolve_arsonist_detonation(&actions, &doused, &roster()), None);
    }

    #[test]
    fn a_guardian_angels_protection_saves_exactly_one_doused_player() {
        let doused = [PlayerId(2), PlayerId(3)];
        let actions = [NightAction::Detonate, NightAction::Protect { target: PlayerId(2) }];
        let deaths = resolve_arsonist_detonation(&actions, &doused, &roster());
        assert_eq!(deaths, Some(vec![(PlayerId(3), KillMethod::Burn)]));
    }

    #[test]
    fn an_already_dead_doused_player_is_skipped() {
        let doused = [PlayerId(2), PlayerId(99)]; // 99 isn't in the roster
        let actions = [NightAction::Detonate];
        let deaths = resolve_arsonist_detonation(&actions, &doused, &roster());
        assert_eq!(deaths, Some(vec![(PlayerId(2), KillMethod::Burn)]));
    }
}

#[cfg(test)]
mod harlot_visit_tests {
    use super::*;

    fn roster() -> Vec<AlivePlayer> {
        vec![
            AlivePlayer { id: PlayerId(1), role: Role::Harlot },
            AlivePlayer { id: PlayerId(2), role: Role::Villager },
            AlivePlayer { id: PlayerId(3), role: Role::Villager },
            AlivePlayer { id: PlayerId(4), role: Role::SerialKiller },
        ]
    }

    #[test]
    fn visiting_the_wolves_actual_target_kills_the_harlot() {
        let harlot = PlayerId(1);
        let victim = PlayerId(2);
        let actions = [NightAction::Visit {
            visitor: harlot,
            target: victim,
        }];
        assert_eq!(
            resolve_harlot_visit_deaths(&actions, Some(victim), &roster()),
            vec![(harlot, KillMethod::VisitWolf)]
        );
    }

    #[test]
    fn visiting_someone_else_is_safe() {
        let harlot = PlayerId(1);
        let elsewhere = PlayerId(3);
        let wolf_target = PlayerId(2);
        let actions = [NightAction::Visit {
            visitor: harlot,
            target: elsewhere,
        }];
        assert_eq!(resolve_harlot_visit_deaths(&actions, Some(wolf_target), &roster()), vec![]);
    }

    #[test]
    fn no_wolf_target_means_no_visit_death() {
        let harlot = PlayerId(1);
        let target = PlayerId(2);
        let actions = [NightAction::Visit {
            visitor: harlot,
            target,
        }];
        assert_eq!(resolve_harlot_visit_deaths(&actions, None, &roster()), vec![]);
    }

    #[test]
    fn no_harlot_in_play_means_no_visit_death() {
        assert_eq!(resolve_harlot_visit_deaths(&[], Some(PlayerId(2)), &roster()), vec![]);
    }

    #[test]
    fn visiting_the_serial_killer_directly_kills_the_harlot() {
        let harlot = PlayerId(1);
        let sk = PlayerId(4);
        let actions = [NightAction::Visit {
            visitor: harlot,
            target: sk,
        }];
        assert_eq!(
            resolve_harlot_visit_deaths(&actions, None, &roster()),
            vec![(harlot, KillMethod::VisitKiller)]
        );
    }

    #[test]
    fn visiting_the_serial_killers_own_target_kills_the_harlot() {
        let harlot = PlayerId(1);
        let sk_target = PlayerId(3);
        let actions = [
            NightAction::SerialKillVote { target: sk_target },
            NightAction::Visit {
                visitor: harlot,
                target: sk_target,
            },
        ];
        assert_eq!(
            resolve_harlot_visit_deaths(&actions, None, &roster()),
            vec![(harlot, KillMethod::VisitVictim)]
        );
    }
}

#[cfg(test)]
mod wolf_cub_bonus_kill_tests {
    use super::*;
    use async_trait::async_trait;

    struct ScriptedWolfPresenter {
        answer: Option<PlayerId>,
        asked: Vec<(PlayerId, Vec<PlayerId>)>,
    }

    #[async_trait(?Send)]
    impl Presenter for ScriptedWolfPresenter {
        async fn ask_targets(
            &mut self,
            player: PlayerId,
            prompt: Prompt,
            options: &[PlayerId],
            count: usize,
        ) -> Option<Vec<PlayerId>> {
            if prompt != Prompt::WolfCubBonusKill || count != 1 {
                return None;
            }
            self.asked.push((player, options.to_vec()));
            self.answer.map(|t| vec![t])
        }
    }

    #[tokio::test]
    async fn every_surviving_wolf_is_asked_for_a_bonus_target() {
        let wolf = PlayerId(1);
        let villager = PlayerId(2);
        let alive = vec![
            AlivePlayer { id: wolf, role: Role::Wolf },
            AlivePlayer { id: villager, role: Role::Villager },
        ];
        let mut presenter = ScriptedWolfPresenter {
            answer: Some(villager),
            asked: vec![],
        };

        let target = resolve_wolf_cub_bonus_kill(&alive, &mut presenter).await;

        assert_eq!(target, Some(villager));
        assert_eq!(presenter.asked.len(), 1);
        assert_eq!(presenter.asked[0].0, wolf);
        assert!(
            !presenter.asked[0].1.contains(&wolf),
            "the bonus kill's options should never include a wolf-team player"
        );
    }

    #[tokio::test]
    async fn declining_the_bonus_kill_kills_nobody() {
        let alive = vec![
            AlivePlayer { id: PlayerId(1), role: Role::Wolf },
            AlivePlayer { id: PlayerId(2), role: Role::Villager },
        ];
        let mut presenter = ScriptedWolfPresenter {
            answer: None,
            asked: vec![],
        };
        assert_eq!(resolve_wolf_cub_bonus_kill(&alive, &mut presenter).await, None);
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
                Prompt::GunnerShoot => self.gunner_shot.map(|t| vec![t]),
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

        let (day_actions, lynch_target) = resolve_day(&players, &mut states, &mut presenter).await;
        assert_eq!(lynch_target, Some(target));

        let mut states2 = HashMap::new();
        let deaths =
            apply_day_results(&day_actions, lynch_target, Some(Role::Villager), &mut states2);
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

        let (day_actions, lynch_target) = resolve_day(&players, &mut states, &mut presenter).await;
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

        let (day_actions, lynch) = resolve_day(&players, &mut states, &mut presenter).await;
        assert_eq!(lynch, Some(lynch_target));
        assert!(day_actions.iter().any(|a| matches!(a, DayAction::Shoot { target, .. } if *target == shot_target)));

        let mut states2 = HashMap::new();
        let mut deaths =
            apply_day_results(&day_actions, lynch, Some(Role::Villager), &mut states2);
        deaths.sort_by_key(|(id, _)| id.0);
        assert_eq!(
            deaths,
            vec![
                (lynch_target, KillMethod::Lynch),
                (shot_target, KillMethod::Shoot),
            ]
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

        let (_day_actions, lynch_target) = resolve_day(&players, &mut states, &mut presenter).await;
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

        let (_day_actions, lynch_target) = resolve_day(&players, &mut states, &mut presenter).await;
        assert_eq!(lynch_target, None);
    }
}
