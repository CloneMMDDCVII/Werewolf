//! Runs a historical fixture game through the real orchestrator
//! (`run_game`, driven by `FixturePresenter` replaying that game's
//! recorded decisions) and writes out a text transcript — one line per
//! message a real Telegram bot would have sent — via `TranscriptPresenter`.
//!
//! Usage: `cargo run -p sim --bin narrate [game_id] [output_path]`
//! With no `game_id`, narrates the first fixture game. Defaults
//! `output_path` to `<game_id>.txt` in the current directory.
use game_engine::orchestrator::AlivePlayer;
use game_engine::roles::PlayerId;
use game_engine::run_game;
use i18n::LanguagePack;
use sim::{load_fixtures, FixturePresenter, TranscriptPresenter};
use std::collections::HashMap;
use std::fs;

const FIXTURE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/games_100.json");
const LANGUAGE_PACK_PATH: &str =
    "/home/user/Werewolf/Werewolf for Telegram/Languages/English.xml";

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let game_id: Option<i64> = args.next().and_then(|s| s.parse().ok());
    let output_path_arg = args.next();

    let games = load_fixtures(FIXTURE_PATH).expect("fixtures should load");
    let fixture = match game_id {
        Some(id) => games
            .iter()
            .find(|g| g.game_id == id)
            .unwrap_or_else(|| panic!("no fixture game with id {id}")),
        None => games.first().expect("at least one fixture game"),
    };

    let pack = LanguagePack::load(LANGUAGE_PACK_PATH).expect("English.xml should load");

    let names: HashMap<PlayerId, String> = fixture
        .players
        .iter()
        .map(|p| (PlayerId(p.telegram_id as u64), p.name.clone()))
        .collect();

    let alive: Vec<AlivePlayer> = fixture
        .players
        .iter()
        .map(|p| AlivePlayer {
            id: PlayerId(p.telegram_id as u64),
            role: p.role,
        })
        .collect();

    let mut fixture_presenter = FixturePresenter::new(fixture, 1);
    let mut transcript = TranscriptPresenter::new(&mut fixture_presenter, &pack, &names);

    transcript.push_line(format!(
        "Werewolf — game {} ({} players, historical winner: {:?})",
        fixture.game_id,
        fixture.players.len(),
        fixture.winner
    ));

    let outcome = run_game(&alive, &mut transcript, 50).await;

    for &(victim, method) in &outcome.deaths {
        transcript.push_death(victim, method);
    }
    transcript.push_line(format!(
        "=== Game over after {} round(s): {:?} (historical: {:?}) ===",
        outcome.rounds_played, outcome.winner, fixture.winner
    ));

    let output_path =
        output_path_arg.unwrap_or_else(|| format!("{}.txt", fixture.game_id));
    fs::write(&output_path, transcript.lines.join("\n") + "\n").expect("write transcript");
    println!("wrote {} lines to {output_path}", transcript.lines.len());
}
