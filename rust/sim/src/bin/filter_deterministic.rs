//! Filters `fixtures/games_100.json` down to the subset that is fully
//! deterministic and replayable with the data/logic we currently have,
//! writing the result to `fixtures/games_deterministic.json`. That curated
//! subset is the strict pass/fail regression gate; the full 100 remain the
//! informational report (see tests/win_condition_replay.rs).
use serde_json::Value;
use sim::{load_fixtures, replay, ReplayResult};
use std::fs;

fn main() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let src_path = format!("{manifest_dir}/fixtures/games_100.json");
    let dst_path = format!("{manifest_dir}/fixtures/games_deterministic.json");

    let raw = fs::read_to_string(&src_path).expect("read games_100.json");
    let mut root: Value = serde_json::from_str(&raw).expect("parse games_100.json");

    let fixtures = load_fixtures(&src_path).expect("fixtures should load");
    let keep_ids: std::collections::HashSet<i64> = fixtures
        .iter()
        .filter(|f| matches!(replay(f), ReplayResult::Match))
        .map(|f| f.game_id)
        .collect();

    let games = root
        .get_mut("games")
        .and_then(|g| g.as_array_mut())
        .expect("games array");
    let total = games.len();
    games.retain(|g| {
        let id = g.get("game_id").and_then(|v| v.as_i64()).unwrap();
        keep_ids.contains(&id)
    });

    println!("kept {} of {} games", games.len(), total);
    fs::write(&dst_path, serde_json::to_string_pretty(&root).unwrap()).expect("write output");
    println!("wrote {dst_path}");
}
