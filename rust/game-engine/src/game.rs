//! The full loop: night, then day, checking for a winner after each,
//! repeating until one is found or a safety cap is hit. This is the first
//! thing in the crate that ties `orchestrator`'s phase resolution to the
//! `evaluate_winner_with_kills` win-condition logic across *multiple*
//! rounds — until now, every proof (`day_orchestration.rs`,
//! `fixture_day_replay.rs`) checked a single night or a single day in
//! isolation.

use crate::freeze::{apply_freeze, ask_freeze};
use crate::messages::Announcement;
use crate::rules::GameRules;
use crate::summary::{is_winner, GameSummary, SummaryEntry};
use crate::orchestrator::{
    apply_day_results, apply_night_results, last_day_veto, resolve_arsonist_detonation, resolve_chemistry,
    resolve_cultist_conversion, resolve_day, resolve_grave_digger_falls, resolve_harlot_visit_deaths,
    resolve_hunter_shots, resolve_lover_deaths, resolve_lynch_vote, resolve_night_by_actor,
    resolve_spumpkin_detonation, resolve_wolf_cub_bonus_kill, AlivePlayer, NarrationEvent, Presenter,
};
use crate::reveals::{resolve_detective, resolve_night_readings, AugurMemory};
use crate::rng::RngSource;
use crate::roles::{behavior_for, DayAction, LoversState, NightAction, PlayerId, RoleState};
use crate::{evaluate_winner_with_kills, is_wolf_muscle, KillEvent, PlayerState, WinOutcome};
use shared::{KillMethod, Role, Team};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameOutcome {
    pub winner: WinOutcome,
    pub rounds_played: u32,
    /// Every death, in the order it happened, with the victim's identity —
    /// `KillEvent` (used internally for win-checking) only carries the
    /// victim's role, which isn't enough to compare a simulated game
    /// against a specific historical one death-for-death.
    pub deaths: Vec<(PlayerId, KillMethod)>,
}

/// Runs the game to completion: night, day, check win, repeat. `max_rounds`
/// is a safety cap, not a rule — a night+day round where both the eat and
/// lynch votes tie produces zero deaths, and without a cap that would loop
/// forever. Hitting the cap returns `WinOutcome::Unimplemented` rather than
/// panicking, since "the vote never resolved" is a real (if unlikely)
/// outcome this proof-of-concept doesn't have a tie-breaker rule for.
///
/// Any `WinOutcome` other than `Team(_)` (i.e. `RngDependent`,
/// `InsufficientData`, `Unimplemented`) is treated as "keep playing," which
/// is honest for `evaluate_common`'s genuine "game continues" cases but
/// **not** for the RNG/data-gap cases — a real game would have resolved
/// those (with a coin flip, say) rather than continuing. That's a known
/// divergence from real play, not a hidden one: those cases already return
/// distinct, named variants specifically so a caller can tell them apart
/// from an actual "no winner yet."
pub async fn run_game(
    players: &[AlivePlayer],
    presenter: &mut dyn Presenter,
    rng: &mut dyn RngSource,
    max_rounds: u32,
) -> GameOutcome {
    let pool = default_role_pool();
    run_game_with_role_pool(players, &pool, presenter, rng, max_rounds).await
}

/// Every role that can currently be dealt, once each. Stands in for the
/// balancer's pool when a caller has none (fixtures, tests); a real game
/// should pass the pool from `balancing::balance_with_pool`.
fn default_role_pool() -> Vec<Role> {
    shared::ALL_ROLES.iter().copied().filter(|&r| behavior_for(r).is_available()).collect()
}

/// `run_game`, with the pool of roles the game was dealt from, which the
/// Augur draws on (see `balancing::balance_with_pool`).
pub async fn run_game_with_role_pool(
    players: &[AlivePlayer],
    role_pool: &[Role],
    presenter: &mut dyn Presenter,
    rng: &mut dyn RngSource,
    max_rounds: u32,
) -> GameOutcome {
    run_game_with_rules(players, role_pool, &GameRules::default(), presenter, rng, max_rounds).await
}

/// What stays fixed for the whole game: the starting roster and the
/// group's rules.
struct Setup<'a> {
    players: &'a [AlivePlayer],
    rules: &'a GameRules,
}

/// `run_game_with_role_pool`, following the group's settings (see
/// `rules::GameRules` and `control::game_rules::rules_for`).
pub async fn run_game_with_rules(
    players: &[AlivePlayer],
    role_pool: &[Role],
    rules: &GameRules,
    presenter: &mut dyn Presenter,
    rng: &mut dyn RngSource,
    max_rounds: u32,
) -> GameOutcome {
    let setup = Setup { players, rules };
    let reveal = rules.show_roles_on_death;
    let mut alive: Vec<AlivePlayer> = players.to_vec();
    let mut states: HashMap<PlayerId, RoleState> = HashMap::new();
    let mut kills: Vec<KillEvent> = vec![];
    let mut deaths: Vec<(PlayerId, KillMethod)> = vec![];
    let mut lovers = LoversState::default();
    // Set by a Blacksmith's SpreadSilver the day before, consumed by the
    // *next* night's apply_night_results call, then reset - cross-round
    // state the same way LoversState is, just a plain bool instead of a
    // whole struct since there's only ever one thing to remember.
    let mut silver_spread_tonight = false;
    // Same shape, for the Wolf Cub's bonus kill (Werewolf.cs:1047-1053):
    // set whenever a Wolf Cub death is recorded, consumed at the start of
    // the *next* night. This mirrors the legacy sequencing precisely
    // (confirmed by reading NightCycle directly, not assumed): the
    // WolfCubKilled flag is only reset at the end of a night's own
    // resolution (Werewolf.cs:3235), *after* that night's menus already
    // went out (Werewolf.cs:3030) - so a wolf answering the main "who do
    // you eat?" question gets chained straight into a second "bonus eat?"
    // question the same night, before either resolves, using whatever
    // was left over from the last death. Both answers are collected and
    // resolved together. An earlier version of this asked the bonus kill
    // as a separate follow-up *after* the main kill had already resolved
    // - the wrong sequencing, caught by checking the source instead of
    // assuming the shape from the mechanic's description alone.
    let mut wolf_cub_bonus_pending = false;
    // Every player the Arsonist has doused, across every night so far
    // (Werewolf.cs: `x.Doused`) - accumulates with no per-night reset
    // until a detonation clears it (or a doused player dies some other
    // way and simply stops being alive to burn). Tracked here rather
    // than in `RoleState`, which only has a single `remembered_player`
    // slot - see `arsonist::Arsonist`'s doc for why that's not enough.
    let mut doused: Vec<PlayerId> = vec![];
    // Every player bitten by the wolves last night (Werewolf.cs:3289,
    // `bitten`), waiting to turn Wolf at the start of *this* night, before
    // any menus go out (Werewolf.cs:2988-2998). See
    // `apply_pending_alpha_bites` and `resolve_wolf_eat_target`'s doc for
    // exactly why this can't just be folded into a same-night death.
    let mut bitten_pending: Vec<PlayerId> = vec![];
    // How much of `deaths` had already happened the last time a Grave
    // Digger dug (Werewolf.cs:5294-5299: `DugGravesLastNight =
    // diedSinceLastGrave.Count`, recomputed fresh every night). The delta
    // against `deaths.len()` at the top of each round *is*
    // "diedSinceLastGrave" - deaths accumulate in recorded order and
    // never shrink, so this needs no separate bookkeeping beyond the one
    // cursor.
    let mut deaths_counted_for_graves: usize = 0;
    // Whoever the Snow Wolf froze last night, who can't be frozen again
    // tonight (Werewolf.cs:5276).
    let mut frozen_last_night: Option<PlayerId> = None;
    let mut augur = AugurMemory::default();

    for round in 1..=max_rounds {
        apply_pending_alpha_bites(&bitten_pending, &mut alive, presenter).await;
        bitten_pending.clear();

        if let Some(outcome) =
            check_for_game_end(round, &setup, presenter, &mut alive, &states, &lovers, &mut kills, &mut deaths).await
        {
            return outcome;
        }

        // Werewolf.cs:5295-5296 excludes Flee/Idle deaths from the count -
        // Flee is reachable now (see apply_fled_players), so this filter
        // is doing real work, not just kept for later.
        let mut dug_graves = deaths[deaths_counted_for_graves..]
            .iter()
            .filter(|&&(_, method)| method != KillMethod::Flee && method != KillMethod::Idle)
            .count() as u32;
        deaths_counted_for_graves = deaths.len();

        let start_of_night = alive.clone();
        let night_seconds = rules.night_seconds(round, &alive);
        presenter.narrate(NarrationEvent::Announce(Announcement::NightTime { seconds: night_seconds })).await;
        let freeze = ask_freeze(&alive, frozen_last_night, silver_spread_tonight, presenter).await;
        let (mut tagged_actions, wolf_target) = resolve_night_by_actor(&alive, &mut states, presenter).await;
        let freeze_outcome = apply_freeze(freeze, &mut tagged_actions, &alive, dug_graves, rng);
        narrate_all(freeze_outcome.events, presenter).await;
        frozen_last_night = freeze_outcome.frozen;
        let frozen_role = freeze_outcome.frozen.and_then(|id| alive.iter().find(|p| p.id == id)).map(|p| p.role);
        if frozen_role == Some(Role::GraveDigger) {
            dug_graves = 0; // tonight's graves were never dug (Werewolf.cs:3163-3168)
        }
        if let Some((snow_wolf, target)) = freeze {
            tagged_actions.push((snow_wolf, NightAction::Freeze { snow_wolf, target }));
        }
        let night_actions: Vec<NightAction> = tagged_actions.iter().map(|&(_, a)| a).collect();
        for action in &night_actions {
            if let NightAction::LinkLovers { a, b } = action {
                lovers.link(*a, *b);
            }
            if let NightAction::Douse { target } = action {
                if !doused.contains(target) {
                    doused.push(*target);
                }
            }
        }
        apply_thief_steals(&night_actions, &mut alive, &mut states, presenter).await;

        let wolf_eat =
            resolve_wolf_eat_target(wolf_target, &night_actions, &mut alive, &mut states, rng, presenter).await;
        if let Some(bitten) = wolf_eat.bitten {
            bitten_pending.push(bitten);
        }

        let mut night_deaths = freeze_outcome.deaths;
        night_deaths.extend(apply_night_results(&night_actions, wolf_eat.effective_target, silver_spread_tonight));
        night_deaths.extend(wolf_eat.extra_death);
        night_deaths.extend(resolve_harlot_visit_deaths(&night_actions, wolf_target, &alive));
        night_deaths.extend(resolve_grave_digger_falls(&night_actions, dug_graves, &alive, rng));
        night_deaths.extend(resolve_chemistry(&night_actions, rng));
        if let Some(burn_deaths) = resolve_arsonist_detonation(&night_actions, &doused, &alive) {
            night_deaths.extend(burn_deaths);
            doused.clear();
        }
        let cult_outcome =
            resolve_cultist_conversion(&night_actions, wolf_target.is_some(), dug_graves, &alive, rng);
        night_deaths.extend(cult_outcome.deaths);
        if let Some(converted) = cult_outcome.transform {
            if let Some(p) = alive.iter_mut().find(|p| p.id == converted) {
                let from = p.role;
                presenter
                    .narrate(NarrationEvent::Transform { player: converted, from, to: Role::Cultist })
                    .await;
                p.role = Role::Cultist;
            }
        }
        if wolf_cub_bonus_pending {
            if let Some(target) = resolve_wolf_cub_bonus_kill(&alive, presenter).await {
                let bonus_eat = resolve_wolf_eat_target(
                    Some(target),
                    &night_actions,
                    &mut alive,
                    &mut states,
                    rng,
                    presenter,
                )
                .await;
                if let Some(bitten) = bonus_eat.bitten {
                    bitten_pending.push(bitten);
                }
                if let Some(target) = bonus_eat.effective_target {
                    night_deaths.push((target, KillMethod::Eat));
                }
                night_deaths.extend(bonus_eat.extra_death);
            }
            wolf_cub_bonus_pending = false;
        }
        wolf_cub_bonus_pending |= process_deaths_and_hunter_shots(
            &night_deaths,
            &mut alive,
            &states,
            &lovers,
            &mut kills,
            &mut deaths,
            reveal,
            presenter,
        )
        .await;

        let readings = resolve_night_readings(
            &tagged_actions,
            &start_of_night,
            &alive,
            freeze_outcome.frozen,
            role_pool,
            &mut augur,
            rng,
        );
        narrate_all(readings, presenter).await;

        if let Some(outcome) =
            check_for_game_end(round, &setup, presenter, &mut alive, &states, &lovers, &mut kills, &mut deaths).await
        {
            return outcome;
        }

        let day_seconds = rules.day_seconds(alive.len());
        presenter.narrate(NarrationEvent::Announce(Announcement::DayTime { seconds: day_seconds })).await;
        presenter.narrate(NarrationEvent::Announce(Announcement::Day { number: round })).await;
        let lynch_seconds = rules.timers.lynch_seconds;
        presenter.narrate(NarrationEvent::Announce(Announcement::LynchTime { seconds: lynch_seconds })).await;
        let (day_actions, lynch) = resolve_day(&alive, &mut states, rules, presenter, rng).await;
        let veto = last_day_veto(&day_actions);
        if veto == Some(DayAction::Trouble) {
            if let Some(troublemaker) = alive.iter().find(|p| p.role == Role::Troublemaker) {
                let announcement = Announcement::TroublemakerDoubleLynch { troublemaker: troublemaker.id };
                presenter.narrate(NarrationEvent::Announce(announcement)).await;
            }
        }
        let lynch_target_role = lynch.target().and_then(|t| alive.iter().find(|p| p.id == t).map(|p| p.role));
        demote_gunner_if_shot_a_wise_elder(&day_actions, &mut alive, presenter).await;
        silver_spread_tonight = day_actions.iter().any(|a| matches!(a, DayAction::SpreadSilver));
        let mut day_deaths = apply_day_results(&day_actions, lynch.target(), lynch_target_role, &mut states);
        day_deaths.extend(resolve_spumpkin_detonation(&day_actions, rng));
        let shot_today: Vec<PlayerId> =
            day_deaths.iter().filter(|&&(_, m)| m != KillMethod::Lynch).map(|&(id, _)| id).collect();
        let snoops = resolve_detective(&day_actions, &alive, &shot_today, rng);
        narrate_all(snoops, presenter).await;
        if veto != Some(DayAction::Pacify) {
            if let Some(announcement) = lynch.announcement() {
                presenter.narrate(NarrationEvent::Announce(announcement)).await;
            }
        }
        wolf_cub_bonus_pending |= process_deaths_and_hunter_shots(
            &day_deaths,
            &mut alive,
            &states,
            &lovers,
            &mut kills,
            &mut deaths,
            reveal,
            presenter,
        )
        .await;

        if let Some(outcome) =
            check_for_game_end(round, &setup, presenter, &mut alive, &states, &lovers, &mut kills, &mut deaths).await
        {
            return outcome;
        }

        // The Troublemaker's second lynch: the whole vote again, with no
        // other day actions (Werewolf.cs:2548-2846).
        if veto == Some(DayAction::Trouble) {
            presenter.narrate(NarrationEvent::Announce(Announcement::TroubleDoubleLynchNow)).await;
            presenter.narrate(NarrationEvent::Announce(Announcement::LynchTime { seconds: lynch_seconds })).await;
            let second = resolve_lynch_vote(&alive, &states, rules, presenter, rng).await;
            let second_role = second.target().and_then(|t| alive.iter().find(|p| p.id == t).map(|p| p.role));
            let second_deaths = apply_day_results(&[], second.target(), second_role, &mut states);
            if let Some(announcement) = second.announcement() {
                presenter.narrate(NarrationEvent::Announce(announcement)).await;
            }
            wolf_cub_bonus_pending |= process_deaths_and_hunter_shots(
                &second_deaths,
                &mut alive,
                &states,
                &lovers,
                &mut kills,
                &mut deaths,
                reveal,
                presenter,
            )
            .await;

            if let Some(outcome) =
                check_for_game_end(round, &setup, presenter, &mut alive, &states, &lovers, &mut kills, &mut deaths).await
            {
                return outcome;
            }
        }

        presenter.advance_round();
    }

    let winner = WinOutcome::Unimplemented(
        "max_rounds reached without a resolved winner (likely repeated tied votes)",
    );
    presenter.narrate(NarrationEvent::GameOver { winner: winner.clone() }).await;
    let summary = game_summary(&setup, &winner, &alive, &lovers, &kills, &deaths);
    presenter.narrate(NarrationEvent::GameSummary(summary)).await;
    GameOutcome {
        winner,
        rounds_played: max_rounds,
        deaths,
    }
}

/// The checkpoint `run_game` runs after every phase: an admin ending the
/// game (`/killgame`), then any flees, then the win check. `Some` means
/// the game is over and has already been narrated as such.
#[allow(clippy::too_many_arguments)]
async fn check_for_game_end(
    round: u32,
    setup: &Setup<'_>,
    presenter: &mut dyn Presenter,
    alive: &mut Vec<AlivePlayer>,
    states: &HashMap<PlayerId, RoleState>,
    lovers: &LoversState,
    kills: &mut Vec<KillEvent>,
    deaths: &mut Vec<(PlayerId, KillMethod)>,
) -> Option<GameOutcome> {
    let winner = if presenter.game_was_killed() {
        WinOutcome::Killed
    } else {
        let reveal = setup.rules.show_roles_on_death;
        apply_fled_players(presenter, alive, states, lovers, kills, deaths, reveal).await;
        resolved_winner(alive, kills, lovers)?
    };
    presenter.narrate(NarrationEvent::GameOver { winner: winner.clone() }).await;
    let summary = game_summary(setup, &winner, alive, lovers, kills, deaths);
    presenter.narrate(NarrationEvent::GameSummary(summary)).await;
    Some(GameOutcome {
        winner,
        rounds_played: round,
        deaths: deaths.clone(),
    })
}

/// Builds the end-of-game summary: survivors first, then the dead in the
/// order they died, each with their final role (`kills` records the role
/// each victim died as, in the same order as `deaths`).
fn game_summary(
    setup: &Setup<'_>,
    winner: &WinOutcome,
    alive: &[AlivePlayer],
    lovers: &LoversState,
    kills: &[KillEvent],
    deaths: &[(PlayerId, KillMethod)],
) -> GameSummary {
    let lynched_tanner = kills.iter().any(|k| k.method == KillMethod::Lynch && k.victim_role == Role::Tanner);
    let in_love = |id: PlayerId| lovers.partner_of(id).is_some();
    let entry = |id: PlayerId, role: Role, alive: bool, fled: bool| SummaryEntry {
        id,
        role,
        alive,
        fled,
        in_love: in_love(id),
        won: is_winner(winner, role, in_love(id), lynched_tanner),
    };
    let mut entries: Vec<SummaryEntry> = setup
        .players
        .iter()
        .filter_map(|p| alive.iter().find(|a| a.id == p.id))
        .map(|a| entry(a.id, a.role, true, false))
        .collect();
    for (&(id, method), kill) in deaths.iter().zip(kills) {
        entries.push(entry(id, kill.victim_role, false, method == KillMethod::Flee));
    }
    GameSummary { end_roles: setup.rules.end_roles, entries }
}

/// Processes a batch of deaths, then every Hunter final shot they
/// trigger, repeatedly: a Hunter shot by a Hunter gets a final shot of
/// their own, and so on down the chain (Werewolf.cs:5480-5481, the
/// "Domino" case). Returns whether a Wolf Cub died anywhere in the chain.
#[allow(clippy::too_many_arguments)]
async fn process_deaths_and_hunter_shots(
    new_deaths: &[(PlayerId, KillMethod)],
    alive: &mut Vec<AlivePlayer>,
    states: &HashMap<PlayerId, RoleState>,
    lovers: &LoversState,
    kills: &mut Vec<KillEvent>,
    deaths: &mut Vec<(PlayerId, KillMethod)>,
    reveal: bool,
    presenter: &mut dyn Presenter,
) -> bool {
    let mut wolf_cub_died = false;
    let mut batch = new_deaths.to_vec();
    while !batch.is_empty() {
        let (processed, died_with_roles) =
            process_deaths(&batch, alive, states, lovers, kills, deaths, reveal, presenter).await;
        wolf_cub_died |= died_with_roles.iter().any(|&(_, role)| role == Role::WolfCub);
        batch = resolve_hunter_shots(&processed, &died_with_roles, alive, presenter).await;
    }
    wolf_cub_died
}

/// Records a batch of deaths into both the win-check kill history and the
/// caller-facing death list, using each victim's role *at the moment they
/// died* — which may already be a transformed role (e.g. a Traitor who
/// turned Wolf in an earlier round) rather than their original assignment.
/// Must be called with `still_alive` from before `.retain()` removes the
/// victims, since that's the only place their pre-death role is available.
/// Returns the same (victim, role-at-death) pairs for `apply_transforms`'
/// Doppelganger case, which needs to know not just *that* its role model
/// died but *what they were*.
fn record_deaths(
    still_alive: &[AlivePlayer],
    new_deaths: &[(PlayerId, KillMethod)],
    kills: &mut Vec<KillEvent>,
    deaths: &mut Vec<(PlayerId, KillMethod)>,
) -> Vec<(PlayerId, Role)> {
    let mut died_with_roles = vec![];
    for &(victim, method) in new_deaths {
        let victim_role = still_alive
            .iter()
            .find(|p| p.id == victim)
            .expect("a death should only ever name a player who was alive to be targeted")
            .role;
        kills.push(KillEvent {
            victim_role,
            method,
        });
        died_with_roles.push((victim, victim_role));
    }
    deaths.extend(new_deaths.iter().copied());
    died_with_roles
}

/// Tells the presenter about every death from this phase, in order, right
/// after they're resolved — not saved up and replayed from `GameOutcome`
/// once the whole game is over. `new_deaths` and `died_with_roles` come
/// from the same `record_deaths` call in the same order, so zipping them
/// is safe; kept as a separate pass (rather than folded into
/// `record_deaths` itself) since "build the win-check bookkeeping" and
/// "tell the presenter" are different concerns that happen to run over
/// the same data.
async fn narrate_deaths(
    new_deaths: &[(PlayerId, KillMethod)],
    died_with_roles: &[(PlayerId, Role)],
    reveal_role: bool,
    presenter: &mut dyn Presenter,
) {
    for (&(victim, method), &(_, role)) in new_deaths.iter().zip(died_with_roles) {
        presenter
            .narrate(NarrationEvent::Death { victim, role, method, reveal_role })
            .await;
    }
}

/// Records, narrates, removes from `alive`, and resolves any transform
/// triggered by one batch of deaths - the full sequence every source of
/// deaths needs (night, day, and now Hunter's revenge shot), pulled into
/// one place instead of copy-pasted a third time.
///
/// Also folds in `resolve_lover_deaths` right at the start, rather than
/// leaving each call site to remember it - Werewolf.cs's lover check
/// lives inside the one, universal `KillPlayer` function
/// (Werewolf.cs:5609-5616), so it applies to *every* death regardless of
/// source, the same way it needs to apply here whether the batch came
/// from a night, a lynch, or a Hunter's revenge shot.
///
/// Returns the full death batch actually processed (original deaths plus
/// any chained lover death) alongside the (victim, role-at-death) pairs,
/// so a caller that needs either for something *else* deaths trigger
/// (`resolve_hunter_shots`, which needs both the method - for which
/// prompt to ask - and the role - to find a Hunter) doesn't have to
/// re-derive them.
#[allow(clippy::too_many_arguments)]
async fn process_deaths(
    new_deaths: &[(PlayerId, KillMethod)],
    alive: &mut Vec<AlivePlayer>,
    states: &HashMap<PlayerId, RoleState>,
    lovers: &LoversState,
    kills: &mut Vec<KillEvent>,
    deaths: &mut Vec<(PlayerId, KillMethod)>,
    reveal_role: bool,
    presenter: &mut dyn Presenter,
) -> (Vec<(PlayerId, KillMethod)>, Vec<(PlayerId, Role)>) {
    let lover_chain = resolve_lover_deaths(new_deaths, lovers, alive);
    let mut all_deaths = new_deaths.to_vec();
    all_deaths.extend(lover_chain);

    let died_with_roles = record_deaths(alive, &all_deaths, kills, deaths);
    narrate_deaths(&all_deaths, &died_with_roles, reveal_role, presenter).await;
    alive.retain(|p| !all_deaths.iter().any(|&(v, _)| v == p.id));
    let transforms = apply_transforms(alive, states, &died_with_roles);
    narrate_all(transforms, presenter).await;
    (all_deaths, died_with_roles)
}

/// Turns whatever `Presenter::drain_fled_players` reports into real
/// deaths via the same `process_deaths` pipeline every other death
/// source uses (Werewolf.cs:848-860: `RemovePlayer`'s `IsRunning`
/// branch calls `CheckRoleChanges` - `apply_transforms`, already inside
/// `process_deaths` - and `DBKill(p, p, KillMthd.Flee)`). A fled id that
/// isn't currently alive (already dead, or never existed) is silently
/// dropped rather than treated as an error - by the time this is
/// polled, "did they already die some other way first" is a race this
/// proof-of-concept resolves by just not re-killing a corpse, the same
/// way a real Telegram presenter would find nothing left to remove.
async fn apply_fled_players(
    presenter: &mut dyn Presenter,
    alive: &mut Vec<AlivePlayer>,
    states: &HashMap<PlayerId, RoleState>,
    lovers: &LoversState,
    kills: &mut Vec<KillEvent>,
    deaths: &mut Vec<(PlayerId, KillMethod)>,
    reveal_role: bool,
) {
    // One player can be reported twice in a window (a `/flee` and an
    // admin's `/smite`, or a smite naming them twice); they still die once.
    let mut seen = HashSet::new();
    let fled_deaths: Vec<(PlayerId, KillMethod)> = presenter
        .drain_fled_players()
        .into_iter()
        .filter(|id| alive.iter().any(|p| p.id == *id) && seen.insert(*id))
        .map(|id| (id, KillMethod::Flee))
        .collect();
    if !fled_deaths.is_empty() {
        process_deaths(&fled_deaths, alive, states, lovers, kills, deaths, reveal_role, presenter).await;
    }
}

/// Tells the presenter about a batch of already-built events, in order.
/// Shared by anything that (unlike `narrate_deaths`) can build its
/// `NarrationEvent`s directly rather than zipping two parallel arrays —
/// `apply_transforms` today, the natural landing spot for anything else
/// that ends up producing a batch of events at once.
async fn narrate_all(events: Vec<NarrationEvent>, presenter: &mut dyn Presenter) {
    for event in events {
        presenter.narrate(event).await;
    }
}

/// The Wise Elder's real defensive quirk (Werewolf.cs:2887-2889): shooting
/// one doesn't save them — they still die from the shot like anyone else
/// (confirmed directly: `KillPlayer` runs unconditionally right after the
/// Wise Elder switch case) — but it demotes the *Gunner* to Villager as a
/// consequence. Checked and applied before `apply_day_results` turns
/// `day_actions` into deaths, using the still-intact `alive` roster to
/// look up the target's role; `DayAction::Shoot`'s `shooter` field is
/// what makes finding the right player to demote a lookup instead of a
/// guess (at most one Gunner exists in practice, but there's no reason to
/// assume that when the action already names who fired).
async fn demote_gunner_if_shot_a_wise_elder(
    day_actions: &[DayAction],
    alive: &mut [AlivePlayer],
    presenter: &mut dyn Presenter,
) {
    let shooter = day_actions.iter().find_map(|a| match a {
        DayAction::Shoot { shooter, target } => {
            let target_role = alive.iter().find(|p| p.id == *target).map(|p| p.role);
            (target_role == Some(Role::WiseElder)).then_some(*shooter)
        }
        _ => None,
    });
    let Some(shooter) = shooter else { return };
    if let Some(gunner) = alive.iter_mut().find(|p| p.id == shooter) {
        let from = gunner.role;
        presenter
            .narrate(NarrationEvent::Transform {
                player: gunner.id,
                from,
                to: Role::Villager,
            })
            .await;
        gunner.role = Role::Villager;
    }
}

/// What happens to a wolf-eat target once `resolve_wolf_eat_target` has
/// checked it against every non-"just die" case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WolfEatOutcome {
    /// The target to actually feed into `apply_night_results` (`None` if
    /// the attempt was fully absorbed by one of the other fields, or
    /// there was no target at all).
    effective_target: Option<PlayerId>,
    /// Set when the target was bitten rather than killed
    /// (Werewolf.cs:3289: `bitten`) — survives *this* night, but
    /// `game::run_game` must turn them into a Wolf at the very start of
    /// the *next* one, before that night's menus go out
    /// (Werewolf.cs:2988-2998, `TransformationMethod.AlphaBitten`, run
    /// during `NightCycle`'s opening per-player reset loop). Named
    /// separately from `effective_target` because both can theoretically
    /// be produced by the same call in principle (they can't today - see
    /// below - but the two are conceptually different outcomes, not the
    /// same field reused).
    bitten: Option<PlayerId>,
    /// A death this call produced *besides* whatever happens to
    /// `effective_target` - today, only ever the wolf a Hunter shoots back
    /// during a standoff (Werewolf.cs:3369: `KillPlayer(shotWuff,
    /// KillMthd.HunterShot, ...)`), which happens *alongside*, not instead
    /// of, the Hunter's own fate.
    extra_death: Option<(PlayerId, KillMethod)>,
}

const NO_EFFECT: WolfEatOutcome = WolfEatOutcome { effective_target: None, bitten: None, extra_death: None };

/// Alpha Wolf's conversion chance (Werewolf.cs `Settings.cs:97,123`:
/// `AlphaWolfConversionChance = 20`) — a per-group setting in the real
/// game; this proof-of-concept has no group settings model, so it always
/// uses the real default rather than inventing a knob nothing reads yet.
const ALPHA_WOLF_CONVERSION_CHANCE_PERCENT: u32 = 20;

/// A Hunter's base chance of shooting back during a wolf-eat standoff
/// (Werewolf.cs `Settings.cs:95,121`: `HunterKillWolfChanceBase = 30`),
/// before `wolf_voter_count` scales it up.
const HUNTER_KILL_WOLF_CHANCE_BASE_PERCENT: u32 = 30;

/// Intercepts the wolves' primary/bonus eat target before it becomes a
/// death, for every case in the wolf-eat resolution switch that isn't
/// "just die" (Werewolf.cs:3289-3420 — shared by both the primary and
/// Wolf Cub bonus kill, since both targets are pushed into the same
/// `choices` list and iterated through this one switch, confirmed by
/// reading Werewolf.cs:3239-3270 directly):
///
/// - Cursed (Werewolf.cs:3298-3299): `Transform(target, IRole.Wolf, ...)`
///   unconditionally, no `KillPlayer` call, no bitten check at all — a
///   wolf-eaten Cursed player joins the wolves outright. Checked *first*,
///   before bitten, since the source's own `switch` reaches this `case`
///   before ever evaluating `bitten` for this target.
/// - Serial Killer dodge (Werewolf.cs:2339-2347) — structurally checked
///   even earlier than Cursed in the real source: it's not part of the
///   per-role `switch` at all, but a gate inside the shared `VisitPlayer`
///   that runs *before* that switch is ever reached. The wolves' chosen
///   visitor only has a 20% chance of surviving a visit to the Serial
///   Killer (`R.Next(100) < 80` is the death condition - easy to misread
///   backwards); an SK with no valid kill target that night has no chance
///   at all. On a visitor death, the SK doesn't die this attempt - the
///   whole per-role switch below (including bitten) is simply never
///   reached, matching `VisitPlayer` returning something other than
///   `Success`.
/// - Hunter standoff (Werewolf.cs:3324-3377), also checked *before*
///   bitten — the source's `case IRole.Hunter` doesn't look at `bitten`
///   at all on its own success path, only falling into the shared bitten
///   check via `goto default` if the standoff itself fails. Chance is
///   `HunterKillWolfChanceBase + (wolf_voter_count - 1) * 20`
///   (`wolf_voter_count` approximated as the number of `EatVote` actions
///   cast, this proof-of-concept's closest equivalent to `voteWolves
///   .Count()`). On success: one random living wolf-team member (picked
///   via `RngSource::pick_index`, mirroring `voteWolves.ElementAt(R
///   .Next(...))`) dies as `KillMethod::HunterShot`; the Hunter *also*
///   dies via a normal `KillMethod::Eat` (`hunterFinalShot: false`,
///   already handled generically by `resolve_hunter_shots` excluding
///   `Eat`) - but only if more than one wolf voted
///   (Werewolf.cs:3355: `if (voteWolves.Count() > 1)`) - a lone wolf who
///   gets shot back at takes the Hunter's target down with them and the
///   Hunter survives unscathed. On failure: falls through to the same
///   bitten check as everyone else.
/// - Bitten (Werewolf.cs:3289: `voteWolves.Any(AlphaWolf) &&
///   R.Next(100) < AlphaWolfConversionChance`): every other role reachable
///   here (`goto default` included - Harlot, Traitor, SerialKiller, a
///   failed Hunter standoff, and the plain fallthrough case all check it)
///   survives the night entirely if bitten, marked for conversion to Wolf
///   at the start of the *next* one instead of dying now. `has_alpha`
///   approximates `voteWolves.Any(AlphaWolf)` as "is any surviving
///   AlphaWolf currently alive" - the real check filters to non-drunk
///   wolves who actually voted, a distinction this proof-of-concept's
///   simplified wolf-eat vote doesn't track.
/// - Wise Elder (Werewolf.cs:3392-3406), only reached when *not* bitten:
///   the *first* non-bitten wolf-eat attempt has no `KillPlayer` call
///   either — `target.HasUsedAbility` is set and the wolves are told they
///   failed. Only a *second* attempt (`HasUsedAbility` already true)
///   falls through to a normal kill (`goto default`). Modeled with
///   `RoleState::primary_used`, the same one-shot flag every other "first
///   time only" role uses.
async fn resolve_wolf_eat_target(
    wolf_target: Option<PlayerId>,
    night_actions: &[NightAction],
    alive: &mut [AlivePlayer],
    states: &mut HashMap<PlayerId, RoleState>,
    rng: &mut dyn RngSource,
    presenter: &mut dyn Presenter,
) -> WolfEatOutcome {
    let Some(target) = wolf_target else { return NO_EFFECT };
    let Some(target_role) = alive.iter().find(|p| p.id == target).map(|p| p.role) else {
        return NO_EFFECT;
    };

    if target_role == Role::Cursed {
        presenter
            .narrate(NarrationEvent::Transform {
                player: target,
                from: Role::Cursed,
                to: Role::Wolf,
            })
            .await;
        if let Some(p) = alive.iter_mut().find(|p| p.id == target) {
            p.role = Role::Wolf;
        }
        return NO_EFFECT;
    }

    if target_role == Role::SerialKiller {
        // Werewolf.cs:2339-2347, VisitPlayer's own gate, checked before the
        // per-role switch even runs - so this isn't "the wolves eat the
        // SK," it's "the wolves' chosen visitor tries to visit the SK and
        // usually doesn't come back." A wolf-team visitor survives only on
        // a 20% roll (`R.Next(100) < 80` is the *death* condition, easy to
        // misread backwards); any non-wolf visitor, or an SK with no valid
        // kill target that night, dies unconditionally with no roll at
        // all - approximated here as "the SK cast no SerialKillVote,"
        // this proof-of-concept's equivalent of `visited.Choice == 0 ||
        // == -1` (`Frozen` isn't tracked at all, so it's simply never
        // true). `wolves` reuses the same "pick a random wolf-team member"
        // approximation as the Hunter standoff above, for the same reason
        // (no per-vote visitor identity tracked). Survival means falling
        // through to the normal switch below, same as everyone else - the
        // SK isn't given any special immunity to bitten/etc, only this one
        // extra gate in front of it.
        let sk_had_target = night_actions.iter().any(|a| matches!(a, NightAction::SerialKillVote { .. }));
        let wolves: Vec<PlayerId> = alive.iter().filter(|p| is_wolf_muscle(p.role)).map(|p| p.id).collect();
        if !wolves.is_empty() && (!sk_had_target || rng.chance(80)) {
            let visitor = wolves[rng.pick_index(wolves.len())];
            return WolfEatOutcome {
                effective_target: None,
                bitten: None,
                extra_death: Some((visitor, KillMethod::VisitKiller)),
            };
        }
    }

    if target_role == Role::Hunter {
        let wolf_voter_count = night_actions.iter().filter(|a| matches!(a, NightAction::EatVote { .. })).count();
        let wolves: Vec<PlayerId> = alive.iter().filter(|p| is_wolf_muscle(p.role)).map(|p| p.id).collect();
        if wolf_voter_count > 0 && !wolves.is_empty() {
            let chance = HUNTER_KILL_WOLF_CHANCE_BASE_PERCENT + (wolf_voter_count as u32 - 1) * 20;
            if rng.chance(chance) {
                let shot_wolf = wolves[rng.pick_index(wolves.len())];
                return WolfEatOutcome {
                    effective_target: (wolf_voter_count > 1).then_some(target),
                    bitten: None,
                    extra_death: Some((shot_wolf, KillMethod::HunterShot)),
                };
            }
        }
    }

    let has_alpha = alive.iter().any(|p| p.role == Role::AlphaWolf);
    if has_alpha && rng.chance(ALPHA_WOLF_CONVERSION_CHANCE_PERCENT) {
        return WolfEatOutcome { effective_target: None, bitten: Some(target), extra_death: None };
    }

    match target_role {
        Role::WiseElder => {
            let state = states.entry(target).or_default();
            if state.primary_used {
                WolfEatOutcome { effective_target: Some(target), bitten: None, extra_death: None }
            } else {
                state.primary_used = true;
                NO_EFFECT
            }
        }
        _ => WolfEatOutcome { effective_target: Some(target), bitten: None, extra_death: None },
    }
}

/// Turns every player bitten last night (Werewolf.cs:2988-2998,
/// `TransformationMethod.AlphaBitten`) into a Wolf, at the very start of
/// this night — before `resolve_night` asks anyone anything, matching the
/// real timing (the transform runs during `NightCycle`'s opening
/// per-player reset loop, before `SendNightActions`). A bitten player who
/// died some other way during the day in between (e.g. lynched) is simply
/// skipped — `Transform` itself no-ops on a dead player for this method
/// (Werewolf.cs:1963-1973 doesn't list `AlphaBitten` among the "works
/// even if dead" exceptions), which falling out of `alive.iter_mut().find`
/// already matches for free. Also skips anyone already wolf-team
/// (Werewolf.cs:2991: `!WolfRoles.Contains(p.PlayerRole) && p.PlayerRole
/// != IRole.SnowWolf`) — `is_wolf_muscle`, not a plain `Role::Wolf`
/// check, since a bitten Wolf Cub/Lycan/SnowWolf shouldn't be "converted"
/// into the one specific role `IRole.Wolf` either (today's wolf-eat never
/// targets a wolf-team player in the first place, so this is a belt-and-
/// suspenders match to the source rather than a reachable case).
async fn apply_pending_alpha_bites(
    bitten_pending: &[PlayerId],
    alive: &mut [AlivePlayer],
    presenter: &mut dyn Presenter,
) {
    for &player in bitten_pending {
        let Some(from) = alive.iter().find(|p| p.id == player).map(|p| p.role) else {
            continue;
        };
        if is_wolf_muscle(from) {
            continue;
        }
        presenter.narrate(NarrationEvent::Transform { player, from, to: Role::Wolf }).await;
        if let Some(p) = alive.iter_mut().find(|p| p.id == player) {
            p.role = Role::Wolf;
        }
    }
}

/// Applies the Thief's steal, immediately, the moment `resolve_night`
/// produces it — unlike Doppelganger's `ChooseRoleModel` (which waits on
/// a death), the swap is unconditional and instant (Werewolf.cs:2483-
/// 2486): the Thief becomes the target's role, and the target becomes a
/// Villager. Applied before `apply_night_results` computes deaths, same
/// timing as extracting Cupid's `LinkLovers` right above this call site -
/// neither depends on who dies tonight.
async fn apply_thief_steals(
    night_actions: &[NightAction],
    alive: &mut [AlivePlayer],
    states: &mut HashMap<PlayerId, RoleState>,
    presenter: &mut dyn Presenter,
) {
    for action in night_actions {
        let NightAction::StealRole { thief, target } = action else {
            continue;
        };
        let target_role = alive.iter().find(|p| p.id == *target).map(|p| p.role);
        let Some(target_role) = target_role else { continue };
        let thief_role = alive.iter().find(|p| p.id == *thief).map(|p| p.role);
        let Some(thief_role) = thief_role else { continue };

        presenter
            .narrate(NarrationEvent::Transform {
                player: *target,
                from: target_role,
                to: Role::Villager,
            })
            .await;
        presenter
            .narrate(NarrationEvent::Transform {
                player: *thief,
                from: thief_role,
                to: target_role,
            })
            .await;

        if let Some(p) = alive.iter_mut().find(|p| p.id == *target) {
            p.role = Role::Villager;
        }
        if let Some(p) = alive.iter_mut().find(|p| p.id == *thief) {
            p.role = target_role;
        }
        // The Thief takes over the role's used abilities and bullets too
        // (Werewolf.cs:2479-2486: `bullet: targetBullet, hasUsedAbility:
        // targetHasUsedAbility`); the new Villager starts fresh.
        let taken = states.remove(target).unwrap_or_default();
        states.insert(*thief, taken);
    }
}

/// Applies role transforms. Most are triggered by the current alive
/// roster's composition rather than by reacting to one specific death
/// event — "no wolf muscle currently alive," "no Seer currently alive,"
/// "role model not in the alive list" are the same check either way, and
/// evaluating them fresh each round is simpler than threading through
/// exactly which death caused it. Doppelganger is the one exception that
/// needs to know more than "did my role model die" — it needs *what they
/// were*, hence `just_died`.
///
/// - Snow Wolf turns Wolf once no other wolf remains alive
///   (Werewolf.cs:4485-4496).
/// - Traitor turns Wolf once no wolf-muscle role remains alive
///   (Werewolf.cs:4499-4512).
/// - Apprentice Seer turns Seer once no Seer remains alive
///   (Werewolf.cs:4053).
/// - Wild Child turns Wolf once her remembered role model (see
///   `roles::wild_child`) is no longer among the alive.
/// - Doppelganger copies whatever role their remembered role model had
///   the moment they died (Werewolf.cs:1936-1937) — not always Wolf,
///   unlike Wild Child.
///
/// Idempotent: once a player's role has been transformed away from the
/// one a given branch matches on, that branch doesn't match them again.
///
/// Returns every transform that happened, as `NarrationEvent`s, for the
/// caller to narrate — same shape as `record_deaths` returning
/// `(PlayerId, Role)` pairs rather than narrating inline: building the
/// event and mutating `player.role` are one step here (the `from` has to
/// be captured before it's overwritten), but *telling* a presenter is a
/// separate concern the caller (`run_game`) owns, same as it does for
/// deaths.
fn apply_transforms(
    alive: &mut [AlivePlayer],
    states: &HashMap<PlayerId, RoleState>,
    just_died: &[(PlayerId, Role)],
) -> Vec<NarrationEvent> {
    let any_wolf_muscle = alive.iter().any(|p| is_wolf_muscle(p.role));
    // The legacy `WolfRoles` (Werewolf.cs:48), which leave out the Snow Wolf.
    let any_eating_wolf = alive
        .iter()
        .any(|p| matches!(p.role, Role::Wolf | Role::AlphaWolf | Role::WolfCub | Role::Lycan));
    let any_seer = alive.iter().any(|p| p.role == Role::Seer);
    let alive_ids: Vec<PlayerId> = alive.iter().map(|p| p.id).collect();
    let mut events = vec![];

    for player in alive.iter_mut() {
        match player.role {
            // The last wolf standing stops freezing and starts eating
            // (Werewolf.cs:4485-4496). Checked before the Traitor, who
            // only turns when no Snow Wolf is left either.
            Role::SnowWolf if !any_eating_wolf => {
                events.push(NarrationEvent::Transform {
                    player: player.id,
                    from: player.role,
                    to: Role::Wolf,
                });
                player.role = Role::Wolf;
            }
            Role::Traitor if !any_wolf_muscle => {
                events.push(NarrationEvent::Transform {
                    player: player.id,
                    from: player.role,
                    to: Role::Wolf,
                });
                player.role = Role::Wolf;
            }
            Role::ApprenticeSeer if !any_seer => {
                events.push(NarrationEvent::Transform {
                    player: player.id,
                    from: player.role,
                    to: Role::Seer,
                });
                player.role = Role::Seer;
            }
            Role::WildChild => {
                if let Some(model) = states.get(&player.id).and_then(|s| s.remembered_player) {
                    if !alive_ids.contains(&model) {
                        events.push(NarrationEvent::Transform {
                            player: player.id,
                            from: player.role,
                            to: Role::Wolf,
                        });
                        player.role = Role::Wolf;
                    }
                }
            }
            Role::Doppelganger => {
                if let Some(model) = states.get(&player.id).and_then(|s| s.remembered_player) {
                    if let Some(&(_, model_role)) = just_died.iter().find(|&&(id, _)| id == model)
                    {
                        events.push(NarrationEvent::Transform {
                            player: player.id,
                            from: player.role,
                            to: model_role,
                        });
                        player.role = model_role;
                    }
                }
            }
            _ => {}
        }
    }
    events
}

/// `Some(outcome)` only for an actual `Team` win — every other `WinOutcome`
/// variant means "don't stop, but also don't pretend this was a clean
/// continue" (see `run_game`'s doc comment on the RNG/data-gap caveat).
/// Only needs the *current* alive roster: `evaluate_winner` filters to
/// `alive` players internally, so dead players' entries would never be
/// looked at anyway.
///
/// Checks a Lovers win *before* the normal team logic below, matching the
/// real precedence (Werewolf.cs:4525-4527: the lovers check is the very
/// first thing `CheckForWin`'s two-player case looks at). Getting this
/// order backwards is a real bug this test setup caught: two mutually
/// in-love Villager survivors would otherwise hit `evaluate_common`'s "no
/// wolves alive" branch and resolve to a plain Village win before the
/// Lovers check ever ran. `evaluate_winner_with_kills` itself has no idea
/// lover pairings exist at all (that's why `sim`'s fixture replay can
/// never verify a Lovers outcome — the historical export has no `InLove`
/// data to give it), so `run_game` is the one place with enough live
/// state to check it. The Tanner-lynch short-circuit inside
/// `evaluate_winner_with_kills` still can't conflict with this ordering:
/// it only ever names someone who just died, never one of the two
/// players a Lovers win would be checking.
fn resolved_winner(alive: &[AlivePlayer], kills: &[KillEvent], lovers: &LoversState) -> Option<WinOutcome> {
    let player_states: Vec<PlayerState> = alive
        .iter()
        .map(|p| PlayerState {
            id: p.id.0,
            role: p.role,
            alive: true,
        })
        .collect();

    // Checked *before* the general team logic below, matching
    // Werewolf.cs:4525-4527 - the lovers check is the very first thing
    // `CheckForWin`'s two-player case looks at, ahead of even the
    // Sorcerer/Tanner/Thief/Doppelgänger tie-check right after it.
    // `evaluate_common`'s "no wolves alive" branch would otherwise call
    // two mutually-in-love Villager survivors a plain Village win, which
    // is the bug this ordering exists to avoid.
    let alive_ids: Vec<PlayerId> = alive.iter().map(|p| p.id).collect();
    if lovers.is_lovers_win(&alive_ids) {
        return Some(WinOutcome::Team(Team::Lovers));
    }

    if let outcome @ WinOutcome::Team(_) = evaluate_winner_with_kills(&player_states, kills) {
        return Some(outcome);
    }

    None
}

#[cfg(test)]
mod wolf_eat_special_case_tests {
    use super::*;
    use crate::rng::ScriptedRngSource;
    use async_trait::async_trait;

    struct NoopPresenter;

    #[async_trait(?Send)]
    impl Presenter for NoopPresenter {
        async fn ask_targets(
            &mut self,
            _player: PlayerId,
            _prompt: crate::orchestrator::Prompt,
            _options: &[PlayerId],
            _count: usize,
        ) -> Option<Vec<PlayerId>> {
            None
        }
    }

    /// No `AlphaWolf` in play in most of these fixtures, so `has_alpha` is
    /// `false` and the bite roll never gets consulted - a `ScriptedRngSource`
    /// with nothing queued would panic if it *were* consulted, which is
    /// exactly the guardrail we want for "this scenario has no Alpha Wolf."
    fn no_alpha_wolf_rng() -> ScriptedRngSource {
        ScriptedRngSource::new()
    }

    #[tokio::test]
    async fn wolf_eating_the_cursed_converts_them_to_wolf_instead_of_killing() {
        let cursed = PlayerId(1);
        let mut alive = vec![AlivePlayer { id: cursed, role: Role::Cursed }];
        let mut states = HashMap::new();
        let mut presenter = NoopPresenter;
        let mut rng = no_alpha_wolf_rng();

        let outcome =
            resolve_wolf_eat_target(Some(cursed), &[], &mut alive, &mut states, &mut rng, &mut presenter).await;

        assert_eq!(outcome, NO_EFFECT, "no death, and no bite, should result from eating the Cursed");
        assert_eq!(alive[0].role, Role::Wolf);
    }

    #[tokio::test]
    async fn wise_elder_survives_the_first_wolf_eat_attempt() {
        let elder = PlayerId(1);
        let mut alive = vec![AlivePlayer { id: elder, role: Role::WiseElder }];
        let mut states = HashMap::new();
        let mut presenter = NoopPresenter;
        let mut rng = no_alpha_wolf_rng();

        let outcome =
            resolve_wolf_eat_target(Some(elder), &[], &mut alive, &mut states, &mut rng, &mut presenter).await;

        assert_eq!(outcome, NO_EFFECT, "the first attempt should be absorbed, no death");
        assert!(states.get(&elder).unwrap().primary_used);
    }

    #[tokio::test]
    async fn wise_elder_dies_on_a_second_wolf_eat_attempt() {
        let elder = PlayerId(1);
        let mut alive = vec![AlivePlayer { id: elder, role: Role::WiseElder }];
        let mut states = HashMap::new();
        states.insert(elder, RoleState { primary_used: true, ..Default::default() });
        let mut presenter = NoopPresenter;
        let mut rng = no_alpha_wolf_rng();

        let outcome =
            resolve_wolf_eat_target(Some(elder), &[], &mut alive, &mut states, &mut rng, &mut presenter).await;

        assert_eq!(
            outcome,
            WolfEatOutcome { effective_target: Some(elder), bitten: None, extra_death: None },
            "a second attempt should go through as a normal kill"
        );
    }

    #[tokio::test]
    async fn every_other_role_is_unaffected() {
        let villager = PlayerId(1);
        let mut alive = vec![AlivePlayer { id: villager, role: Role::Villager }];
        let mut states = HashMap::new();
        let mut presenter = NoopPresenter;
        let mut rng = no_alpha_wolf_rng();

        let outcome =
            resolve_wolf_eat_target(Some(villager), &[], &mut alive, &mut states, &mut rng, &mut presenter).await;

        assert_eq!(outcome, WolfEatOutcome { effective_target: Some(villager), bitten: None, extra_death: None });
        assert_eq!(alive[0].role, Role::Villager);
    }

    #[tokio::test]
    async fn no_alpha_wolf_alive_means_no_bite_is_even_rolled() {
        // A ScriptedRngSource with an unused answer would fail differently
        // (leftover-answers aren't checked), so what this test actually
        // proves is the *absence* of a panic from an empty queue - if
        // `has_alpha` were wrongly `true` here, the unscripted `chance()`
        // call would panic.
        let target = PlayerId(1);
        let mut alive = vec![AlivePlayer { id: target, role: Role::Seer }];
        let mut states = HashMap::new();
        let mut presenter = NoopPresenter;
        let mut rng = no_alpha_wolf_rng();

        let outcome =
            resolve_wolf_eat_target(Some(target), &[], &mut alive, &mut states, &mut rng, &mut presenter).await;

        assert_eq!(outcome, WolfEatOutcome { effective_target: Some(target), bitten: None, extra_death: None });
    }

    #[tokio::test]
    async fn a_successful_bite_replaces_the_kill_with_a_pending_transform() {
        let target = PlayerId(1);
        let alpha = PlayerId(2);
        let mut alive = vec![
            AlivePlayer { id: target, role: Role::Seer },
            AlivePlayer { id: alpha, role: Role::AlphaWolf },
        ];
        let mut states = HashMap::new();
        let mut presenter = NoopPresenter;
        let mut rng = ScriptedRngSource::new().with_chance(true);

        let outcome =
            resolve_wolf_eat_target(Some(target), &[], &mut alive, &mut states, &mut rng, &mut presenter).await;

        assert_eq!(outcome, WolfEatOutcome { effective_target: None, bitten: Some(target), extra_death: None });
        assert_eq!(alive[0].role, Role::Seer, "no immediate transform - that's next night's job");
    }

    #[tokio::test]
    async fn a_failed_bite_roll_falls_through_to_a_normal_kill() {
        let target = PlayerId(1);
        let alpha = PlayerId(2);
        let mut alive = vec![
            AlivePlayer { id: target, role: Role::Seer },
            AlivePlayer { id: alpha, role: Role::AlphaWolf },
        ];
        let mut states = HashMap::new();
        let mut presenter = NoopPresenter;
        let mut rng = ScriptedRngSource::new().with_chance(false);

        let outcome =
            resolve_wolf_eat_target(Some(target), &[], &mut alive, &mut states, &mut rng, &mut presenter).await;

        assert_eq!(outcome, WolfEatOutcome { effective_target: Some(target), bitten: None, extra_death: None });
    }

    #[tokio::test]
    async fn cursed_bypasses_the_bite_roll_entirely() {
        // Even with an AlphaWolf alive, Cursed's case is checked (and
        // returns) before the bitten roll - an unscripted ScriptedRngSource
        // would panic if this were wrong.
        let cursed = PlayerId(1);
        let alpha = PlayerId(2);
        let mut alive = vec![
            AlivePlayer { id: cursed, role: Role::Cursed },
            AlivePlayer { id: alpha, role: Role::AlphaWolf },
        ];
        let mut states = HashMap::new();
        let mut presenter = NoopPresenter;
        let mut rng = no_alpha_wolf_rng();

        let outcome =
            resolve_wolf_eat_target(Some(cursed), &[], &mut alive, &mut states, &mut rng, &mut presenter).await;

        assert_eq!(outcome, NO_EFFECT);
        assert_eq!(alive[0].role, Role::Wolf);
    }

    fn hunter_roster() -> Vec<AlivePlayer> {
        vec![
            AlivePlayer { id: PlayerId(1), role: Role::Hunter },
            AlivePlayer { id: PlayerId(2), role: Role::Wolf },
        ]
    }

    #[tokio::test]
    async fn a_lone_wolf_shot_back_at_dies_alone_and_the_hunter_survives() {
        let hunter = PlayerId(1);
        let wolf = PlayerId(2);
        let mut alive = hunter_roster();
        let mut states = HashMap::new();
        let mut presenter = NoopPresenter;
        // One EatVote -> wolf_voter_count == 1 -> standoff chance succeeds,
        // then pick_index(1) picks the only wolf.
        let mut rng = ScriptedRngSource::new().with_chance(true).with_index(0);
        let actions = [NightAction::EatVote { target: hunter }];

        let outcome =
            resolve_wolf_eat_target(Some(hunter), &actions, &mut alive, &mut states, &mut rng, &mut presenter).await;

        assert_eq!(
            outcome,
            WolfEatOutcome { effective_target: None, bitten: None, extra_death: Some((wolf, KillMethod::HunterShot)) },
            "with only one wolf voting, the hunter should survive - only the shot wolf dies"
        );
    }

    #[tokio::test]
    async fn multiple_wolves_shooting_back_costs_the_hunter_their_life_too() {
        let hunter = PlayerId(1);
        let wolf_a = PlayerId(2);
        let wolf_b = PlayerId(3);
        let mut alive = vec![
            AlivePlayer { id: hunter, role: Role::Hunter },
            AlivePlayer { id: wolf_a, role: Role::Wolf },
            AlivePlayer { id: wolf_b, role: Role::AlphaWolf },
        ];
        let mut states = HashMap::new();
        let mut presenter = NoopPresenter;
        let actions =
            [NightAction::EatVote { target: hunter }, NightAction::EatVote { target: hunter }];
        let mut rng = ScriptedRngSource::new().with_chance(true).with_index(0);

        let outcome =
            resolve_wolf_eat_target(Some(hunter), &actions, &mut alive, &mut states, &mut rng, &mut presenter).await;

        assert_eq!(outcome.effective_target, Some(hunter), "more than one wolf voting means the hunter also dies");
        assert!(outcome.extra_death.is_some(), "a wolf should still have been shot");
    }

    #[tokio::test]
    async fn a_failed_standoff_falls_through_to_a_normal_kill() {
        let hunter = PlayerId(1);
        let mut alive = hunter_roster();
        let mut states = HashMap::new();
        let mut presenter = NoopPresenter;
        let actions = [NightAction::EatVote { target: hunter }];
        let mut rng = ScriptedRngSource::new().with_chance(false);

        let outcome =
            resolve_wolf_eat_target(Some(hunter), &actions, &mut alive, &mut states, &mut rng, &mut presenter).await;

        assert_eq!(
            outcome,
            WolfEatOutcome { effective_target: Some(hunter), bitten: None, extra_death: None },
            "a failed standoff is a normal kill, with no AlphaWolf around to trigger a bite"
        );
    }

    #[tokio::test]
    async fn no_wolf_votes_at_all_skips_the_standoff_roll_entirely() {
        let hunter = PlayerId(1);
        let mut alive = hunter_roster();
        let mut states = HashMap::new();
        let mut presenter = NoopPresenter;
        // No EatVote actions - an unscripted ScriptedRngSource would panic
        // if the standoff roll were (wrongly) attempted anyway.
        let mut rng = no_alpha_wolf_rng();

        let outcome =
            resolve_wolf_eat_target(Some(hunter), &[], &mut alive, &mut states, &mut rng, &mut presenter).await;

        assert_eq!(outcome, WolfEatOutcome { effective_target: Some(hunter), bitten: None, extra_death: None });
    }

    fn sk_roster() -> Vec<AlivePlayer> {
        vec![
            AlivePlayer { id: PlayerId(1), role: Role::SerialKiller },
            AlivePlayer { id: PlayerId(2), role: Role::Wolf },
        ]
    }

    #[tokio::test]
    async fn a_wolf_visiting_the_sk_usually_dies_instead_of_the_sk() {
        let sk = PlayerId(1);
        let wolf = PlayerId(2);
        let mut alive = sk_roster();
        let mut states = HashMap::new();
        let mut presenter = NoopPresenter;
        let actions = [NightAction::SerialKillVote { target: PlayerId(99) }];
        // sk_had_target is true (a real vote was cast), so the death roll
        // is consulted, and it's scripted to succeed (die).
        let mut rng = ScriptedRngSource::new().with_chance(true).with_index(0);

        let outcome =
            resolve_wolf_eat_target(Some(sk), &actions, &mut alive, &mut states, &mut rng, &mut presenter).await;

        assert_eq!(
            outcome,
            WolfEatOutcome { effective_target: None, bitten: None, extra_death: Some((wolf, KillMethod::VisitKiller)) }
        );
    }

    #[tokio::test]
    async fn a_lucky_wolf_surviving_the_sk_visit_falls_through_to_a_normal_kill() {
        let sk = PlayerId(1);
        let mut alive = sk_roster();
        let mut states = HashMap::new();
        let mut presenter = NoopPresenter;
        let actions = [NightAction::SerialKillVote { target: PlayerId(99) }];
        let mut rng = ScriptedRngSource::new().with_chance(false);

        let outcome =
            resolve_wolf_eat_target(Some(sk), &actions, &mut alive, &mut states, &mut rng, &mut presenter).await;

        assert_eq!(outcome, WolfEatOutcome { effective_target: Some(sk), bitten: None, extra_death: None });
    }

    #[tokio::test]
    async fn an_sk_with_no_valid_kill_target_always_costs_the_visiting_wolf() {
        let sk = PlayerId(1);
        let wolf = PlayerId(2);
        let mut alive = sk_roster();
        let mut states = HashMap::new();
        let mut presenter = NoopPresenter;
        // No SerialKillVote action at all - the death roll should never
        // even be consulted (unconditional death), matching
        // `visited.Choice == 0 || == -1` in the source.
        let mut rng = ScriptedRngSource::new().with_index(0);

        let outcome =
            resolve_wolf_eat_target(Some(sk), &[], &mut alive, &mut states, &mut rng, &mut presenter).await;

        assert_eq!(
            outcome,
            WolfEatOutcome { effective_target: None, bitten: None, extra_death: Some((wolf, KillMethod::VisitKiller)) }
        );
    }
}

#[cfg(test)]
mod alpha_bite_transform_tests {
    use super::*;
    use async_trait::async_trait;

    struct RecordingPresenter {
        transforms: Vec<NarrationEvent>,
    }

    #[async_trait(?Send)]
    impl Presenter for RecordingPresenter {
        async fn ask_targets(
            &mut self,
            _player: PlayerId,
            _prompt: crate::orchestrator::Prompt,
            _options: &[PlayerId],
            _count: usize,
        ) -> Option<Vec<PlayerId>> {
            None
        }

        async fn narrate(&mut self, event: NarrationEvent) {
            self.transforms.push(event);
        }
    }

    #[tokio::test]
    async fn a_bitten_player_turns_wolf_at_the_start_of_the_next_night() {
        let bitten = PlayerId(1);
        let mut alive = vec![AlivePlayer { id: bitten, role: Role::Seer }];
        let mut presenter = RecordingPresenter { transforms: vec![] };

        apply_pending_alpha_bites(&[bitten], &mut alive, &mut presenter).await;

        assert_eq!(alive[0].role, Role::Wolf);
        assert_eq!(
            presenter.transforms,
            vec![NarrationEvent::Transform { player: bitten, from: Role::Seer, to: Role::Wolf }]
        );
    }

    #[tokio::test]
    async fn a_bitten_player_who_died_in_between_is_skipped() {
        let mut alive: Vec<AlivePlayer> = vec![]; // the bitten player is no longer alive
        let mut presenter = RecordingPresenter { transforms: vec![] };

        apply_pending_alpha_bites(&[PlayerId(1)], &mut alive, &mut presenter).await;

        assert!(presenter.transforms.is_empty());
    }

    #[tokio::test]
    async fn nothing_pending_means_no_transforms() {
        let mut alive = vec![AlivePlayer { id: PlayerId(1), role: Role::Seer }];
        let mut presenter = RecordingPresenter { transforms: vec![] };

        apply_pending_alpha_bites(&[], &mut alive, &mut presenter).await;

        assert_eq!(alive[0].role, Role::Seer);
        assert!(presenter.transforms.is_empty());
    }
}

#[cfg(test)]
mod fled_player_tests {
    use super::*;
    use async_trait::async_trait;

    struct FleeingPresenter {
        pending_fled: Vec<PlayerId>,
        events: Vec<NarrationEvent>,
    }

    #[async_trait(?Send)]
    impl Presenter for FleeingPresenter {
        async fn ask_targets(
            &mut self,
            _player: PlayerId,
            _prompt: crate::orchestrator::Prompt,
            _options: &[PlayerId],
            _count: usize,
        ) -> Option<Vec<PlayerId>> {
            None
        }

        async fn narrate(&mut self, event: NarrationEvent) {
            self.events.push(event);
        }

        fn drain_fled_players(&mut self) -> Vec<PlayerId> {
            std::mem::take(&mut self.pending_fled)
        }
    }

    #[tokio::test]
    async fn a_fled_player_dies_by_flee_and_is_removed() {
        let fled = PlayerId(1);
        let mut alive = vec![
            AlivePlayer { id: fled, role: Role::Villager },
            AlivePlayer { id: PlayerId(2), role: Role::Seer },
        ];
        let states = HashMap::new();
        let lovers = LoversState::default();
        let mut kills = vec![];
        let mut deaths = vec![];
        let mut presenter = FleeingPresenter { pending_fled: vec![fled], events: vec![] };

        apply_fled_players(&mut presenter, &mut alive, &states, &lovers, &mut kills, &mut deaths, true).await;

        assert_eq!(alive.len(), 1);
        assert_eq!(alive[0].id, PlayerId(2));
        assert_eq!(deaths, vec![(fled, KillMethod::Flee)]);
        assert!(presenter
            .events
            .contains(&NarrationEvent::Death {
                victim: fled,
                role: Role::Villager,
                method: KillMethod::Flee,
                reveal_role: true
            }));
    }

    #[tokio::test]
    async fn fleeing_a_lover_kills_their_partner_too() {
        let fled = PlayerId(1);
        let partner = PlayerId(2);
        let mut alive = vec![
            AlivePlayer { id: fled, role: Role::Villager },
            AlivePlayer { id: partner, role: Role::Seer },
        ];
        let states = HashMap::new();
        let mut lovers = LoversState::default();
        lovers.link(fled, partner);
        let mut kills = vec![];
        let mut deaths = vec![];
        let mut presenter = FleeingPresenter { pending_fled: vec![fled], events: vec![] };

        apply_fled_players(&mut presenter, &mut alive, &states, &lovers, &mut kills, &mut deaths, true).await;

        assert!(alive.is_empty());
        assert!(deaths.contains(&(partner, KillMethod::LoverDied)));
    }

    #[tokio::test]
    async fn no_fled_players_touches_nothing() {
        let mut alive = vec![AlivePlayer { id: PlayerId(1), role: Role::Villager }];
        let states = HashMap::new();
        let lovers = LoversState::default();
        let mut kills = vec![];
        let mut deaths = vec![];
        let mut presenter = FleeingPresenter { pending_fled: vec![], events: vec![] };

        apply_fled_players(&mut presenter, &mut alive, &states, &lovers, &mut kills, &mut deaths, true).await;

        assert_eq!(alive.len(), 1);
        assert!(deaths.is_empty());
        assert!(presenter.events.is_empty());
    }

    #[tokio::test]
    async fn a_fled_id_thats_already_dead_is_silently_ignored() {
        let mut alive = vec![AlivePlayer { id: PlayerId(2), role: Role::Villager }];
        let states = HashMap::new();
        let lovers = LoversState::default();
        let mut kills = vec![];
        let mut deaths = vec![];
        // PlayerId(1) isn't in `alive` at all - already dead or never existed.
        let mut presenter = FleeingPresenter { pending_fled: vec![PlayerId(1)], events: vec![] };

        apply_fled_players(&mut presenter, &mut alive, &states, &lovers, &mut kills, &mut deaths, true).await;

        assert_eq!(alive.len(), 1);
        assert!(deaths.is_empty());
    }
}

#[cfg(test)]
mod transform_tests {
    use super::*;

    #[test]
    fn traitor_turns_wolf_once_no_wolf_muscle_remains() {
        let mut alive = vec![
            AlivePlayer {
                id: PlayerId(1),
                role: Role::Traitor,
            },
            AlivePlayer {
                id: PlayerId(2),
                role: Role::Villager,
            },
        ];
        let events = apply_transforms(&mut alive, &HashMap::new(), &[]);
        assert_eq!(alive[0].role, Role::Wolf);
        assert_eq!(
            events,
            vec![NarrationEvent::Transform {
                player: PlayerId(1),
                from: Role::Traitor,
                to: Role::Wolf,
            }]
        );
    }

    #[test]
    fn traitor_stays_a_traitor_while_a_wolf_is_still_alive() {
        let mut alive = vec![
            AlivePlayer {
                id: PlayerId(1),
                role: Role::Traitor,
            },
            AlivePlayer {
                id: PlayerId(2),
                role: Role::Wolf,
            },
        ];
        apply_transforms(&mut alive, &HashMap::new(), &[]);
        assert_eq!(alive[0].role, Role::Traitor);
    }

    #[test]
    fn wild_child_turns_wolf_once_her_role_model_is_gone() {
        let wild_child = PlayerId(1);
        let mut alive = vec![AlivePlayer {
            id: wild_child,
            role: Role::WildChild,
        }];
        let mut states = HashMap::new();
        states.insert(
            wild_child,
            RoleState {
                remembered_player: Some(PlayerId(99)), // not in `alive`
                ..Default::default()
            },
        );
        apply_transforms(&mut alive, &states, &[]);
        assert_eq!(alive[0].role, Role::Wolf);
    }

    #[test]
    fn wild_child_stays_herself_while_her_role_model_is_still_alive() {
        let wild_child = PlayerId(1);
        let role_model = PlayerId(2);
        let mut alive = vec![
            AlivePlayer {
                id: wild_child,
                role: Role::WildChild,
            },
            AlivePlayer {
                id: role_model,
                role: Role::Villager,
            },
        ];
        let mut states = HashMap::new();
        states.insert(
            wild_child,
            RoleState {
                remembered_player: Some(role_model),
                ..Default::default()
            },
        );
        apply_transforms(&mut alive, &states, &[]);
        assert_eq!(alive[0].role, Role::WildChild);
    }

    #[test]
    fn apprentice_seer_becomes_seer_once_no_seer_remains() {
        let mut alive = vec![
            AlivePlayer {
                id: PlayerId(1),
                role: Role::ApprenticeSeer,
            },
            AlivePlayer {
                id: PlayerId(2),
                role: Role::Villager,
            },
        ];
        apply_transforms(&mut alive, &HashMap::new(), &[]);
        assert_eq!(alive[0].role, Role::Seer);
    }

    #[test]
    fn apprentice_seer_stays_apprentice_while_the_seer_lives() {
        let mut alive = vec![
            AlivePlayer {
                id: PlayerId(1),
                role: Role::ApprenticeSeer,
            },
            AlivePlayer {
                id: PlayerId(2),
                role: Role::Seer,
            },
        ];
        apply_transforms(&mut alive, &HashMap::new(), &[]);
        assert_eq!(alive[0].role, Role::ApprenticeSeer);
    }

    #[test]
    fn doppelganger_copies_the_dead_role_models_actual_role() {
        let doppelganger = PlayerId(1);
        let role_model = PlayerId(2);
        let mut alive = vec![AlivePlayer {
            id: doppelganger,
            role: Role::Doppelganger,
        }];
        let mut states = HashMap::new();
        states.insert(
            doppelganger,
            RoleState {
                remembered_player: Some(role_model),
                ..Default::default()
            },
        );
        let just_died = [(role_model, Role::SerialKiller)];
        let events = apply_transforms(&mut alive, &states, &just_died);
        assert_eq!(alive[0].role, Role::SerialKiller);
        assert_eq!(
            events,
            vec![NarrationEvent::Transform {
                player: doppelganger,
                from: Role::Doppelganger,
                to: Role::SerialKiller,
            }]
        );
    }

    #[test]
    fn doppelganger_stays_itself_if_someone_else_died() {
        let doppelganger = PlayerId(1);
        let role_model = PlayerId(2);
        let mut alive = vec![AlivePlayer {
            id: doppelganger,
            role: Role::Doppelganger,
        }];
        let mut states = HashMap::new();
        states.insert(
            doppelganger,
            RoleState {
                remembered_player: Some(role_model),
                ..Default::default()
            },
        );
        let just_died = [(PlayerId(3), Role::SerialKiller)]; // not the role model
        apply_transforms(&mut alive, &states, &just_died);
        assert_eq!(alive[0].role, Role::Doppelganger);
    }
}
