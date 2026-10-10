//! Proves the live night, driven by a `FixturePresenter` instead of a
//! test-local scripted one, reproduces actual historical outcomes: not
//! just "the final winner matches" (that's `win_condition_replay.rs`), but
//! "the wolves' night-1 vote, asked by `resolve_night_with_freeze`, comes
//! out the same as what really happened, and resolving the night with
//! `night::resolve_night_sequence` kills that player by `Eat`."

use game_engine::night::{resolve_night_sequence, NightInput, NightMemory};
use game_engine::orchestrator::{resolve_night_with_freeze, AlivePlayer};
use game_engine::rng::SeededRngSource;
use game_engine::roles::{PlayerId, RoleState};
use shared::{KillMethod, Role};
use sim::{load_fixtures, FixturePresenter, GameFixture};
use std::collections::HashMap;

const FIXTURE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/games_100.json");

/// The historical night-1 Eat victim, if this game recorded one **and**
/// the killer held `Role::Wolf` specifically: the fixtures record only
/// plain wolves' votes faithfully enough to replay.
fn historical_night1_eat_by_plain_wolf(game: &GameFixture) -> Option<i64> {
    let kill = game.kills.iter().find(|k| k.method == KillMethod::Eat && k.day == 1)?;
    let killer_is_plain_wolf = game
        .players
        .iter()
        .any(|p| p.telegram_id == kill.killer_telegram_id && p.role == Role::Wolf);
    killer_is_plain_wolf.then_some(kill.victim_telegram_id)
}

#[tokio::test]
async fn resolve_night_reproduces_the_historical_night1_wolf_kill() {
    let games = load_fixtures(FIXTURE_PATH).expect("fixtures should load");

    let mut checked = 0;
    for game in &games {
        let Some(expected_victim) = historical_night1_eat_by_plain_wolf(game) else {
            continue; // no night-1 plain-Wolf kill in this game, nothing to check
        };

        let players: Vec<AlivePlayer> = game
            .players
            .iter()
            .map(|p| AlivePlayer {
                id: PlayerId(p.telegram_id as u64),
                role: p.role,
            })
            .collect();

        let mut presenter = FixturePresenter::new(game, 1);
        let mut states: HashMap<PlayerId, RoleState> = HashMap::new();
        let (actions, wolf_target, _) =
            resolve_night_with_freeze(&players, &mut states, None, &mut presenter, &[], &[]).await;

        assert_eq!(
            wolf_target,
            Some(PlayerId(expected_victim as u64)),
            "game {}: orchestrator's resolved wolf target should match history",
            game.game_id
        );

        let input = NightInput {
            night: 1,
            actions: &actions,
            wolf_cub_votes: &[],
            frozen: None,
            saved_from_freeze: None,
            dug_graves: 0,
            thief_full: false,
            reveal: true,
            role_pool: &[],
        };
        let mut alive = players.clone();
        let night = resolve_night_sequence(
            &input,
            &mut alive,
            &mut states,
            &mut NightMemory::default(),
            &mut SeededRngSource::new(0),
        );
        assert!(
            night
                .deaths
                .iter()
                .any(|d| d.victim == PlayerId(expected_victim as u64) && d.method == KillMethod::Eat),
            "game {}: resolving the night should kill the historical victim by Eat: {:?}",
            game.game_id,
            night.deaths
        );

        checked += 1;
    }

    assert!(
        checked > 0,
        "expected at least one fixture game with a recorded night-1 wolf kill to check against"
    );
    println!("checked {checked} games with a historical night-1 wolf kill");
}
