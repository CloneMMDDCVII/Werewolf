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
/// has one. Deliberately narrow today: the legacy `*Killed` keys
/// (`GunnerKilled`, `SeerKilled`, `PrinceKilled`, ...) are all
/// Serial-Killer-specific flavor text, not general per-role death
/// messages (confirmed by reading their actual English.xml values — every
/// one mentions "the serial killer has struck again"), so wiring those up
/// honestly requires modeling *which* role died *and* that the method was
/// `SerialKilled` specifically, which is future work, not this slice.
/// For now: the real `LynchKill` text for lynches, and `None` (caller
/// falls back to a generic line) for everything else — matching exactly
/// what `sim::TranscriptPresenter` was already doing, just relocated here
/// so it's the one place this mapping is declared instead of living
/// inside one specific `Presenter` implementation. `KillMethod::LoverDied`
/// is one of the "everything else" cases for the same reason as
/// `HunterShot`: the real `LoverDied` text needs *two* names (the lover
/// who died first, and the one dying of grief) plus a role reveal, and
/// `NarrationEvent::Death` only carries one victim - giving it real
/// flavor text needs that event to grow a second identity field first.
pub fn death_locale_key(_role: Role, method: KillMethod) -> Option<&'static str> {
    match method {
        KillMethod::Lynch => Some("LynchKill"),
        _ => None,
    }
}

/// Maps a resolved transform to its real legacy flavor key. Only the
/// transforms `game::apply_transforms` actually models have one:
/// Traitor→Wolf (`TraitorTurnWolf`), ApprenticeSeer→Seer
/// (`ApprenticeNowSeer`), WildChild→Wolf (`WildChildTransform`), and
/// Doppelganger copying a dead role model into one of the four roles the
/// legacy game has dedicated flavor text for (`DGToWolf`/`DGToCult`/
/// `DGToMason`/`DGToSnowWolf`) — a Doppelganger copying any other role
/// has no dedicated key in English.xml (checked directly), so falls
/// through to `None` same as everything else uncovered.
pub fn transform_locale_key(from: Role, to: Role) -> Option<&'static str> {
    match (from, to) {
        (Role::Traitor, Role::Wolf) => Some("TraitorTurnWolf"),
        (Role::ApprenticeSeer, Role::Seer) => Some("ApprenticeNowSeer"),
        (Role::WildChild, Role::Wolf) => Some("WildChildTransform"),
        (Role::Doppelganger, Role::Wolf) => Some("DGToWolf"),
        (Role::Doppelganger, Role::Cultist) => Some("DGToCult"),
        (Role::Doppelganger, Role::Mason) => Some("DGToMason"),
        (Role::Doppelganger, Role::SnowWolf) => Some("DGToSnowWolf"),
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
    keys.push("LynchKill"); // death_locale_key(_, KillMethod::Lynch)
    keys.extend([
        // transform_locale_key's non-`None` outputs.
        "TraitorTurnWolf",
        "ApprenticeNowSeer",
        "WildChildTransform",
        "DGToWolf",
        "DGToCult",
        "DGToMason",
        "DGToSnowWolf",
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
        Role::AlphaWolf | Role::WolfCub | Role::Lycan | Role::SnowWolf => Some(Prompt::WolfEat),
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
/// of those would incorrectly grant a shot here. Also doesn't model
/// "Domino" chaining (a Hunter's shot landing on *another* Hunter, who
/// would get their own final shot in turn, Werewolf.cs:5480-5481) — this
/// only asks each already-dead Hunter from the triggering batch once, not
/// recursively.
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
        let new_deaths = [(hunter, KillMethod::Eat)];
        let died_with_roles = [(hunter, Role::Hunter)];

        resolve_hunter_shots(&new_deaths, &died_with_roles, &alive_roster(), &mut presenter).await;

        assert_eq!(presenter.asked[0].1, Prompt::HunterFinalShotKilled);
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
