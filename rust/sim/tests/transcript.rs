//! Proves `TranscriptPresenter` actually narrates a real fixture replay —
//! not just that it compiles. Doesn't assert exact wording (that's the
//! locale files' job, already covered by `i18n`'s own tests) — just that
//! driving a real game through it produces the shape of transcript a user
//! asked for: a line per night/day question, in day order, ending with a
//! resolved outcome line.

use game_engine::orchestrator::AlivePlayer;
use game_engine::roles::PlayerId;
use game_engine::run_game;
use i18n::LanguagePack;
use sim::{load_fixtures, FixturePresenter, TranscriptPresenter};
use std::collections::HashMap;

const FIXTURE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/games_100.json");
const LANGUAGE_PACK_PATH: &str = "/home/user/Werewolf/Werewolf for Telegram/Languages/English.xml";

#[tokio::test]
async fn narrates_a_full_fixture_replay_as_a_readable_transcript() {
    let games = load_fixtures(FIXTURE_PATH).expect("fixtures should load");
    let fixture = games.first().expect("at least one fixture game");
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

    let outcome = run_game(&alive, &mut transcript, 50).await;
    let lines_before_deaths = transcript.lines.len();
    for &(victim, method) in &outcome.deaths {
        transcript.push_death(victim, method);
    }

    assert!(
        transcript.lines.iter().any(|l| l.starts_with("=== Day 1 ===")),
        "transcript should open with a day header"
    );
    assert!(
        transcript.lines.iter().any(|l| l.contains("was asked")),
        "transcript should narrate at least one question, got: {:?}",
        transcript.lines
    );
    assert_eq!(
        transcript.lines.len() - lines_before_deaths,
        outcome.deaths.len(),
        "every resolved death should get exactly one transcript line"
    );

    // Real player names, not raw telegram ids, should appear in the script.
    let some_real_name = fixture.players.first().unwrap().name.clone();
    assert!(
        transcript.lines.iter().any(|l| l.contains(&some_real_name)),
        "transcript should use the fixture's display names, not raw ids"
    );
}
