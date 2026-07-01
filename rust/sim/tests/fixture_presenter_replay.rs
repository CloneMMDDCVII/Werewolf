//! Proves the real orchestrator, driven by a `FixturePresenter` instead of
//! a test-local scripted one, reproduces actual historical outcomes — not
//! just "the final winner matches" (that's `win_condition_replay.rs`), but
//! "the wolves' night-1 kill tally, computed fresh by resolve_night, comes
//! out the same as what really happened," and now "applying that tally
//! via apply_night_results produces the same death that history recorded."
//! None of these 100 games include a Witch (she's not a legacy role), so
//! there's never a heal to cancel the kill here — this only proves the
//! unconditional-death path, not the heal-cancels-it path (that's already
//! covered by orchestrator's own unit tests, which don't need real fixture
//! data to check a rule that never happened historically).

use game_engine::orchestrator::{apply_night_results, resolve_night, AlivePlayer};
use game_engine::roles::{PlayerId, RoleState};
use shared::{KillMethod, Role};
use sim::{load_fixtures, FixturePresenter, GameFixture};
use std::collections::HashMap;

const FIXTURE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/games_100.json");

/// The historical night-1 Eat victim, if this game recorded one **and**
/// the killer held `Role::Wolf` specifically. Chaos-mode wolf variants
/// (WolfCub, AlphaWolf, Lycan, SnowWolf) aren't ported into
/// `game-engine`'s role behaviors yet (see `behavior_for`'s `Unimplemented`
/// list), so a game whose wolf was one of those can't be reproduced by the
/// orchestrator — that's a scope boundary to route around in this test,
/// not something to silently over-claim by checking against it anyway.
fn historical_night1_eat_by_plain_wolf(game: &GameFixture) -> Option<i64> {
    let kill = game
        .kills
        .iter()
        .find(|k| k.method == KillMethod::Eat && k.day == 1)?;
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
        let (actions, wolf_target) = resolve_night(&players, &mut states, &mut presenter).await;

        assert_eq!(
            wolf_target,
            Some(PlayerId(expected_victim as u64)),
            "game {}: orchestrator's resolved wolf target should match history",
            game.game_id
        );

        let deaths = apply_night_results(&actions, wolf_target, false);
        assert_eq!(
            deaths,
            vec![(PlayerId(expected_victim as u64), KillMethod::Eat)],
            "game {}: applying the resolved night should produce exactly the historical death",
            game.game_id
        );

        checked += 1;
    }

    assert!(
        checked > 0,
        "expected at least one fixture game with a recorded night-1 wolf kill to check against"
    );
    println!("checked {checked} games with a historical night-1 wolf kill");
}
