//! Regression gate for engine refactors: plays seeded random games, with
//! random rosters, answers and rules, and compares a digest of every
//! game's full transcript (each question, each narration event, the
//! outcome) against `fixtures/engine_transcript_digests.txt`.
//!
//! A refactor that changes nothing keeps every digest. A change that is
//! meant to alter play changes some; check which seeds moved, then rewrite
//! the file with
//!
//! ```text
//! WEREWOLF_BLESS=1 cargo test -p sim --test engine_transcripts
//! ```
//!
//! Renaming a field of a narrated type changes every digest too, since the
//! transcript is its `Debug` output; bless that the same way.
//!
//! To compare two engine versions in full rather than by digest, set
//! `DIFF_OUT=<file>` (and optionally `DIFF_SEEDS=<n>`, default 3000) to write
//! the transcripts themselves, run on both versions, and `cmp` the files.

use async_trait::async_trait;
use game_engine::orchestrator::{AlivePlayer, NarrationEvent, Presenter, Prompt};
use game_engine::rng::SeededRngSource;
use game_engine::roles::PlayerId;
use game_engine::rules::{EndRoles, GameRules, SecretLynch};
use game_engine::run_game_with_rules;
use shared::ALL_ROLES;
use std::fmt::Write;

const DIGESTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/engine_transcript_digests.txt");
const GATE_SEEDS: u64 = 300;

/// xorshift64: the test's own randomness, independent of the engine's.
struct Xorshift(u64);

impl Xorshift {
    fn below(&mut self, bound: usize) -> usize {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x as usize) % bound
    }
}

/// Answers at random (sometimes nothing, sometimes a skip), sends the odd
/// flee, and writes everything it sees.
struct Recorder {
    rng: Xorshift,
    log: String,
    players: usize,
}

#[async_trait(?Send)]
impl Presenter for Recorder {
    async fn ask_targets(
        &mut self,
        player: PlayerId,
        prompt: Prompt,
        options: &[PlayerId],
        count: usize,
    ) -> Option<Vec<PlayerId>> {
        writeln!(self.log, "ASK {player:?} {prompt:?} {options:?} {count}").unwrap();
        let roll = self.rng.below(10);
        if roll == 0 || options.len() < count {
            return None;
        }
        if roll == 1 {
            return Some(vec![]);
        }
        let mut pool = options.to_vec();
        Some((0..count).map(|_| pool.remove(self.rng.below(pool.len()))).collect())
    }

    async fn ask_toggle(&mut self, player: PlayerId, prompt: Prompt) -> bool {
        writeln!(self.log, "TOGGLE {player:?} {prompt:?}").unwrap();
        self.rng.below(3) == 0
    }

    async fn narrate(&mut self, event: NarrationEvent) {
        writeln!(self.log, "EV {event:?}").unwrap();
    }

    fn advance_round(&mut self) {
        writeln!(self.log, "ROUND").unwrap();
    }

    fn drain_fled_players(&mut self) -> Vec<PlayerId> {
        if self.rng.below(40) == 0 {
            vec![PlayerId(1 + self.rng.below(self.players) as u64)]
        } else {
            vec![]
        }
    }
}

/// One seeded game's whole transcript.
async fn transcript(seed: u64) -> String {
    let mut r = Xorshift(seed);
    let n = 5 + r.below(16);
    let players: Vec<AlivePlayer> = (0..n)
        .map(|i| AlivePlayer {
            id: PlayerId(i as u64 + 1),
            role: ALL_ROLES[r.below(ALL_ROLES.len())],
        })
        .collect();
    let rules = GameRules {
        thief_full: r.below(2) == 0,
        show_roles_on_death: r.below(3) != 0,
        secret_lynch: (r.below(3) == 0).then(|| SecretLynch {
            show_votes: r.below(2) == 0,
            show_voters: r.below(2) == 0,
        }),
        end_roles: [EndRoles::None, EndRoles::Living, EndRoles::All][r.below(3)],
        ..GameRules::default()
    };
    let mut presenter = Recorder {
        rng: Xorshift(seed.wrapping_mul(2_654_435_761).max(1)),
        log: String::new(),
        players: n,
    };
    let mut rng = SeededRngSource::new(seed);
    let outcome = run_game_with_rules(&players, ALL_ROLES, &rules, &mut presenter, &mut rng, 40).await;
    format!(
        "=== seed {seed} {players:?} {rules:?}\n{}OUTCOME {outcome:?}\n",
        presenter.log
    )
}

/// FNV-1a, so the digest never depends on the standard library's hasher.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

#[tokio::test]
async fn transcripts_match_the_committed_digests() {
    let mut digests = String::new();
    for seed in 1..=GATE_SEEDS {
        writeln!(digests, "{seed} {:016x}", fnv1a(&transcript(seed).await)).unwrap();
    }
    if std::env::var_os("WEREWOLF_BLESS").is_some() {
        std::fs::write(DIGESTS, &digests).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(DIGESTS).expect("digest file should exist; bless it first");
    let moved: Vec<&str> = digests
        .lines()
        .zip(committed.lines())
        .filter(|(now, before)| now != before)
        .map(|(now, _)| now.split(' ').next().unwrap())
        .collect();
    assert!(
        moved.is_empty() && digests.lines().count() == committed.lines().count(),
        "{} of {GATE_SEEDS} games play differently (seeds {moved:?}). If that is intended, \
         rerun with WEREWOLF_BLESS=1 (see this file's header).",
        moved.len()
    );
}

#[tokio::test]
async fn full_transcripts_on_request() {
    let Some(path) = std::env::var_os("DIFF_OUT") else {
        return;
    };
    let seeds: u64 = std::env::var("DIFF_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);
    let mut out = String::new();
    for seed in 1..=seeds {
        out.push_str(&transcript(seed).await);
    }
    std::fs::write(path, out).unwrap();
}
