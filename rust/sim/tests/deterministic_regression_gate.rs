use sim::{load_fixtures, replay, ReplayResult};

const FIXTURE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/games_deterministic.json");

/// Strict regression gate: this fixture set was curated (via
/// `bin/filter_deterministic.rs`) to contain only games where the win
/// condition is fully deterministic from the data we export — no RNG
/// branches, no transformable roles, no missing InLove/Bullet state. Every
/// game here MUST match; a failure means a real engine regression, not a
/// known data/logic gap.
#[test]
fn all_deterministic_fixtures_match() {
    let games = load_fixtures(FIXTURE_PATH).expect("deterministic fixtures should load");
    assert!(!games.is_empty(), "curated fixture set should not be empty");

    let mut failures = vec![];
    for g in &games {
        match replay(g) {
            ReplayResult::Match => {}
            ReplayResult::Mismatch { expected, got } => {
                failures.push(format!("game {}: expected {expected:?}, got {got:?}", g.game_id))
            }
            ReplayResult::NotEvaluated(reason) => failures.push(format!(
                "game {}: curated as deterministic but engine says NotEvaluated: {reason}",
                g.game_id
            )),
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} curated games failed:\n{}",
        failures.len(),
        games.len(),
        failures.join("\n")
    );
}
