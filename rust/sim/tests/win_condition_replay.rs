use sim::{load_fixtures, replay, ReplayResult};

const FIXTURE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/games_100.json");

/// Reports win-condition match rate across all fixtures. This is a report,
/// not a pass/fail gate yet — the engine only covers the deterministic
/// majority-path branches (see game-engine/src/lib.rs). Mismatches are
/// printed so we know exactly what still needs porting; any *actual*
/// mismatch (not "not evaluated") fails the test, since that means our
/// ported logic disagrees with history on a case it claims to handle.
#[test]
fn win_condition_replay_report() {
    let games = load_fixtures(FIXTURE_PATH).expect("fixtures should load");

    let mut matches = 0;
    let mut mismatches = vec![];
    let mut not_evaluated: Vec<(i64, &'static str)> = vec![];

    for g in &games {
        match replay(g) {
            ReplayResult::Match => matches += 1,
            ReplayResult::Mismatch { expected, got } => {
                mismatches.push((g.game_id, expected, got));
            }
            ReplayResult::NotEvaluated(reason) => not_evaluated.push((g.game_id, reason)),
        }
    }

    println!(
        "\nwin-condition replay: {matches}/{} matched, {} not evaluated, {} MISMATCHED\n",
        games.len(),
        not_evaluated.len(),
        mismatches.len()
    );
    for (id, expected, got) in &mismatches {
        println!("  MISMATCH game {id}: expected {expected:?}, engine said {got:?}");
    }
    for (id, reason) in &not_evaluated {
        println!("  not evaluated game {id}: {reason}");
    }

    assert!(
        mismatches.is_empty(),
        "{} game(s) disagreed with the engine on a case it claims to handle",
        mismatches.len()
    );
}
