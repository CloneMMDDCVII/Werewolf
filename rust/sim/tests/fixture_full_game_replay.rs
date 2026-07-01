//! The multi-round counterpart to `fixture_presenter_replay.rs` (night-1
//! only) and `fixture_day_replay.rs` (day-1 only): drives `run_game`
//! itself through a `FixturePresenter` that advances across rounds,
//! checking whether the real orchestrator's simulated deaths match
//! history for as many consecutive rounds as this codebase's current
//! coverage can honestly support.
//!
//! **Scope, stated up front**: no fixture game is pure Villager+Wolf (a
//! real check against the actual data, not an assumption) — every one
//! includes roles or death mechanics we haven't ported (Harlot/GuardianAngel
//! dying while visiting, Hunter standoffs, cult conversion, transformable
//! roles, and 30 of 45 roles with no behavior at all). Reproducing an
//! *entire* historical game end-to-end isn't achievable honestly yet. What
//! is achievable: for however many consecutive days a game's recorded
//! kills are *exclusively* a plain-Wolf `Eat` or a `Lynch` — the two
//! mechanics `run_game` actually models — the orchestrator should
//! reproduce those deaths exactly. The moment a day contains any other
//! kill method, that's the edge of what we can currently verify, and the
//! test stops comparing there rather than pretending otherwise.

use game_engine::orchestrator::AlivePlayer;
use game_engine::roles::PlayerId;
use game_engine::run_game;
use shared::{KillMethod, Role};
use sim::{load_fixtures, FixturePresenter, GameFixture};

const FIXTURE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/games_100.json");

/// The number of consecutive days (starting at 1) whose kills are entirely
/// explained by mechanics `run_game` models: a plain-`Role::Wolf` `Eat`,
/// or a single-victim `Lynch`. Stops at the first day containing anything
/// else, the first day with no kills at all recorded (can't distinguish
/// "nothing happened" from "something happened we don't have a method
/// for"), or — a real finding from running this test, not an assumption
/// made in advance — the first day where the legacy game recorded *two
/// different* lynch victims on the same day counter (some games re-vote
/// within a day; `run_game`'s model is one lynch per day, so that's
/// outside current scope too).
fn reproducible_day_window(game: &GameFixture) -> u32 {
    let max_day = game.kills.iter().map(|k| k.day).max().unwrap_or(0);

    for day in 1..=max_day {
        let kills_this_day: Vec<_> = game.kills.iter().filter(|k| k.day == day).collect();
        if kills_this_day.is_empty() {
            return day - 1;
        }
        let all_reproducible = kills_this_day.iter().all(|k| match k.method {
            KillMethod::Lynch => true,
            KillMethod::Eat => game
                .players
                .iter()
                .any(|p| p.telegram_id == k.killer_telegram_id && p.role == Role::Wolf),
            _ => false,
        });
        let lynch_victims: std::collections::HashSet<_> = kills_this_day
            .iter()
            .filter(|k| k.method == KillMethod::Lynch)
            .map(|k| k.victim_telegram_id)
            .collect();
        if !all_reproducible || lynch_victims.len() > 1 {
            return day - 1;
        }
    }
    max_day
}

#[tokio::test]
async fn run_game_reproduces_history_for_the_reproducible_day_window() {
    let games = load_fixtures(FIXTURE_PATH).expect("fixtures should load");

    let mut checked_games = 0;
    let mut checked_days = 0;

    for game in &games {
        let window = reproducible_day_window(game);
        if window == 0 {
            continue; // nothing in this game is within our current coverage
        }

        let players: Vec<AlivePlayer> = game
            .players
            .iter()
            .map(|p| AlivePlayer {
                id: PlayerId(p.telegram_id as u64),
                role: p.role,
            })
            .collect();

        let mut presenter = FixturePresenter::new(game, 1);
        let outcome = run_game(&players, &mut presenter, window).await;

        // Compare only up through however many rounds run_game actually
        // played: max_rounds=window caps it, but a win can resolve earlier
        // (e.g. the alive composition alone satisfies a win condition,
        // independent of the death mechanics this test restricts itself
        // to) - that's a legitimate early stop, not a test bug, so the
        // comparison window shrinks to match rather than over-claiming.
        let compared_through = outcome.rounds_played.min(window);
        let expected_deaths: Vec<(PlayerId, KillMethod)> = game
            .kills
            .iter()
            .filter(|k| {
                k.day <= compared_through && matches!(k.method, KillMethod::Eat | KillMethod::Lynch)
            })
            .map(|k| (PlayerId(k.victim_telegram_id as u64), k.method))
            .collect();

        let mut simulated: Vec<_> = outcome.deaths.clone();
        simulated.sort_by_key(|(id, _)| id.0);
        let mut expected = expected_deaths;
        expected.sort_by_key(|(id, _)| id.0);
        expected.dedup(); // multiple GameKill rows can share a victim (multiple lynchers' "credit")

        assert_eq!(
            simulated, expected,
            "game {}: simulated deaths over the {} reproducible day(s) should match history",
            game.game_id, window
        );

        checked_games += 1;
        checked_days += window;
    }

    assert!(
        checked_games > 0,
        "expected at least one fixture game with a reproducible day window"
    );
    println!(
        "checked {checked_games} games across {checked_days} total reproducible day-rounds"
    );
}
