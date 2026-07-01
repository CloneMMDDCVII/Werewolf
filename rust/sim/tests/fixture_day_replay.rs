//! Day-phase counterpart to `fixture_presenter_replay.rs`: proves
//! `resolve_day`, driven by real history via `FixturePresenter`, tallies
//! the same lynch victim history recorded — and that `apply_day_results`
//! turns that into the same death.

use game_engine::orchestrator::{apply_day_results, resolve_day, AlivePlayer};
use game_engine::roles::{PlayerId, RoleState};
use shared::KillMethod;
use sim::{load_fixtures, FixturePresenter, GameFixture};
use std::collections::HashMap;

const FIXTURE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/games_100.json");

fn historical_day1_lynch_victim(game: &GameFixture) -> Option<i64> {
    game.kills
        .iter()
        .find(|k| k.method == KillMethod::Lynch && k.day == 1)
        .map(|k| k.victim_telegram_id)
}

/// Whoever the wolves ate on night 1 is already dead by the day-1 lynch
/// vote and shouldn't be counted as a voter.
fn night1_eat_victim(game: &GameFixture) -> Option<i64> {
    game.kills
        .iter()
        .find(|k| k.method == KillMethod::Eat && k.day == 1)
        .map(|k| k.victim_telegram_id)
}

#[tokio::test]
async fn resolve_day_reproduces_the_historical_day1_lynch() {
    let games = load_fixtures(FIXTURE_PATH).expect("fixtures should load");

    let mut checked = 0;
    for game in &games {
        let Some(expected_victim) = historical_day1_lynch_victim(game) else {
            continue;
        };
        let already_dead = night1_eat_victim(game);

        let players: Vec<AlivePlayer> = game
            .players
            .iter()
            .filter(|p| Some(p.telegram_id) != already_dead)
            .map(|p| AlivePlayer {
                id: PlayerId(p.telegram_id as u64),
                role: p.role,
            })
            .collect();

        let mut presenter = FixturePresenter::new(game, 1);
        let mut states: HashMap<PlayerId, RoleState> = HashMap::new();
        let (day_actions, lynch_target) = resolve_day(&players, &mut states, &mut presenter).await;

        assert_eq!(
            lynch_target,
            Some(PlayerId(expected_victim as u64)),
            "game {}: orchestrator's resolved lynch target should match history",
            game.game_id
        );

        let lynch_target_role = lynch_target.and_then(|t| players.iter().find(|p| p.id == t).map(|p| p.role));
        let deaths = apply_day_results(&day_actions, lynch_target, lynch_target_role, &mut states);
        assert!(
            deaths.contains(&(PlayerId(expected_victim as u64), KillMethod::Lynch)),
            "game {}: applying the resolved day should include the historical lynch death: {deaths:?}",
            game.game_id
        );

        checked += 1;
    }

    assert!(
        checked > 0,
        "expected at least one fixture game with a recorded day-1 lynch to check against"
    );
    println!("checked {checked} games with a historical day-1 lynch");
}
