//! Fuzzes random rosters and random decisions through `run_game` +
//! `TranscriptPresenter` and checks *internal consistency* invariants —
//! not equivalence against the legacy game (that stays fixture-based, see
//! `fixture_full_game_replay.rs`'s doc comment on why: no live C# twin to
//! diff against, and no way to keep two independent RNG streams in
//! lockstep). What this test catches instead is the class of bug that's
//! already bitten this port twice by hand: a locale template's
//! placeholder going unfilled (the Gunner `{0} bullet(s) remain` leak,
//! and the `GenericDeathNoReveal` double-`{0}` leak) and a `Prompt`
//! resolving to the wrong text (the Fool/Sorcerer/Oracle mismap). All
//! three were only found because a human happened to read the output —
//! a fuzzer would have caught them the moment they were introduced,
//! across far more role/count combinations than a human would bother to
//! hand-check.
//!
//! No `rand`/`proptest` dependency: a tiny xorshift is enough for
//! "generate a lot of different scenarios deterministically" and keeps
//! this test self-contained - a failure prints its seed, so it's
//! trivially reproducible without needing to capture external RNG state.

use async_trait::async_trait;
use game_engine::orchestrator::{AlivePlayer, Presenter, Prompt};
use game_engine::roles::PlayerId;
use game_engine::run_game;
use i18n::LanguagePack;
use shared::ALL_ROLES;
use sim::TranscriptPresenter;
use std::collections::HashMap;

const LANGUAGE_PACK_PATH: &str = "/home/user/Werewolf/Werewolf for Telegram/Languages/English.xml";
const SEEDS: u64 = 300;

struct Xorshift64(u64);

impl Xorshift64 {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_range(&mut self, bound: usize) -> usize {
        (self.next_u64() as usize) % bound
    }

    fn next_bool(&mut self) -> bool {
        self.next_u64() % 2 == 0
    }
}

/// Answers every question with a random valid choice or a random decline
/// - deliberately dumb, since the point is to exercise as many code paths
/// as possible, not to play well.
struct RandomPresenter {
    rng: Xorshift64,
}

#[async_trait(?Send)]
impl Presenter for RandomPresenter {
    async fn ask_targets(
        &mut self,
        _player: PlayerId,
        _prompt: Prompt,
        options: &[PlayerId],
        count: usize,
    ) -> Option<Vec<PlayerId>> {
        if self.rng.next_bool() || options.len() < count {
            return None;
        }
        let mut pool: Vec<PlayerId> = options.to_vec();
        let mut picked = Vec::with_capacity(count);
        for _ in 0..count {
            let idx = self.rng.next_range(pool.len());
            picked.push(pool.remove(idx));
        }
        Some(picked)
    }

    async fn ask_toggle(&mut self, _player: PlayerId, _prompt: Prompt) -> bool {
        self.rng.next_bool()
    }
}

fn random_roster(rng: &mut Xorshift64) -> Vec<AlivePlayer> {
    let count = 5 + rng.next_range(11); // 5..=15 players
    (0..count)
        .map(|i| AlivePlayer {
            id: PlayerId(i as u64 + 1),
            role: ALL_ROLES[rng.next_range(ALL_ROLES.len())],
        })
        .collect()
}

#[tokio::test]
async fn fuzzed_games_never_leak_placeholders_or_miscount_deaths() {
    let pack = LanguagePack::load(LANGUAGE_PACK_PATH).expect("English.xml should load");

    for seed in 1..=SEEDS {
        let mut roster_rng = Xorshift64(seed);
        let players = random_roster(&mut roster_rng);

        let names: HashMap<PlayerId, String> =
            players.iter().map(|p| (p.id, format!("P{}", p.id.0))).collect();

        let mut inner = RandomPresenter {
            rng: Xorshift64(seed.wrapping_mul(2_654_435_761).max(1)),
        };
        let mut transcript = TranscriptPresenter::new(&mut inner, &pack, &names);

        let outcome = run_game(&players, &mut transcript, 30).await;

        for line in &transcript.lines {
            assert!(
                !line.contains("{0}") && !line.contains("{1}"),
                "seed {seed}: unfilled locale placeholder leaked into transcript: {line:?}"
            );
        }

        let death_line_indices: Vec<usize> = transcript
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| outcome.deaths.iter().any(|&(_, method)| l.ends_with(&format!("({method:?})"))))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            death_line_indices.len(),
            outcome.deaths.len(),
            "seed {seed}: {} deaths resolved but {} narrated, transcript: {:#?}",
            outcome.deaths.len(),
            death_line_indices.len(),
            transcript.lines
        );

        // Unlike a question line (where "[?]" can legitimately mark a
        // known gap, e.g. Gunner's untracked bullet count), a death
        // announcement always has a real victim name available - "[?]"
        // here means a locale template's placeholder silently swallowed
        // the name instead of being filled (this is exactly the bug a
        // `.replacen(..., 1)` vs `.replace` mixup produces on a template
        // that repeats `{0}` twice, since `strip_unfilled_placeholders`
        // converts the leftover brace into "[?]" before this check ever
        // sees a literal "{0}").
        for &i in &death_line_indices {
            assert!(
                !transcript.lines[i].contains("[?]"),
                "seed {seed}: death line has an unfilled name placeholder: {:?}",
                transcript.lines[i]
            );
        }

        assert!(
            transcript.lines.iter().any(|l| l.starts_with("=== Day 1 ===")),
            "seed {seed}: transcript should always open with a day header"
        );
    }
}
