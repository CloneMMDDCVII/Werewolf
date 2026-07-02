//! The seam every real coin-flip in Werewolf.cs goes through, mirrored as
//! two named decisions rather than a raw `gen_range` — the same reasoning
//! behind `NightAction`/`Prompt` being enums instead of strings. Almost
//! every RNG use in the legacy source is one of two shapes:
//!
//! - `Program.R.Next(100) < chance` — some percent-chance roll
//!   (`AlphaWolfConversionChance`, Chemist's brew, Spumpkin's 40%,
//!   GraveDigger's fall formula, Cultist conversion, ...). See
//!   [`RngSource::chance`].
//! - `xs.ElementAt(Program.R.Next(xs.Count()))` — uniformly picking one
//!   of several equally-likely players (which wolf gets credited with a
//!   bite, which wolf gets shot back in a Hunter standoff, ...). See
//!   [`RngSource::pick_index`].
//!
//! Threaded through `game::run_game` and the `orchestrator` functions
//! that need a roll the same way `Presenter` already is: a `&mut dyn
//! RngSource` parameter, not a global/thread-local, so a test can swap in
//! [`ScriptedRngSource`] and assert both branches of a roll without
//! depending on statistics.
//!
//! **Fixture-replay implication, stated up front so nobody "fixes" a test
//! by chasing it:** the historical fixture games in `sim` were recorded
//! from the real C# `System.Random` stream, which this port has no way to
//! reproduce (different algorithm, different seed, and the seed itself
//! was never recorded). Any round that hits an RNG branch is therefore
//! fundamentally unreplayable byte-for-byte against real history — not a
//! bug to fix, a property of the data. That's exactly why
//! `fixture_full_game_replay.rs` already scopes itself to a "reproducible
//! day window" rather than a whole game; RNG-driven mechanics belong in
//! the simulation/fuzz side of the test suite (`SeededRngSource`,
//! deterministic *within* a run but not tied to any real history), never
//! the strict fixture-replay side.

pub trait RngSource {
    /// Returns `true` with probability `percent_chance` out of 100.
    /// `percent_chance` is clamped to `0..=100` by every implementation
    /// here (a formula producing e.g. 104 should always succeed, never
    /// panic) — mirrors `Program.R.Next(100) < chance` tolerating
    /// `chance` values above 100 (Werewolf.cs's fall-chance formula can
    /// exceed 100 for a heavily-dug grave).
    fn chance(&mut self, percent_chance: u32) -> bool;

    /// Uniformly picks an index in `0..len`. Panics if `len == 0` — every
    /// call site is expected to check for a nonempty candidate list first
    /// (matching `Program.R.Next(xs.Count())`, which throws on an empty
    /// sequence in the original .NET code too, always guarded by an
    /// `.Any()` check beforehand).
    fn pick_index(&mut self, len: usize) -> usize;
}

/// Real randomness (`rand::thread_rng()`), for actual play.
pub struct ThreadRngSource(rand::rngs::ThreadRng);

impl Default for ThreadRngSource {
    fn default() -> Self {
        Self(rand::thread_rng())
    }
}

impl RngSource for ThreadRngSource {
    fn chance(&mut self, percent_chance: u32) -> bool {
        chance_from(&mut self.0, percent_chance)
    }

    fn pick_index(&mut self, len: usize) -> usize {
        pick_index_from(&mut self.0, len)
    }
}

/// Deterministic *within a run* (`rand::rngs::StdRng`, seeded), for
/// reproducible simulation and fuzzing — not for matching real history,
/// see the module doc.
pub struct SeededRngSource(rand::rngs::StdRng);

impl SeededRngSource {
    pub fn new(seed: u64) -> Self {
        use rand::SeedableRng;
        Self(rand::rngs::StdRng::seed_from_u64(seed))
    }
}

impl RngSource for SeededRngSource {
    fn chance(&mut self, percent_chance: u32) -> bool {
        chance_from(&mut self.0, percent_chance)
    }

    fn pick_index(&mut self, len: usize) -> usize {
        pick_index_from(&mut self.0, len)
    }
}

fn chance_from(rng: &mut impl rand::Rng, percent_chance: u32) -> bool {
    rng.gen_range(0..100) < percent_chance.min(100)
}

fn pick_index_from(rng: &mut impl rand::Rng, len: usize) -> usize {
    assert!(len > 0, "pick_index called with no candidates to pick from");
    rng.gen_range(0..len)
}

/// A fixed, in-order queue of canned answers — the `RngSource` equivalent
/// of `ScriptedHunterPresenter`/`ScriptedWolfPresenter` in
/// `orchestrator`'s own test modules. Lets a unit test assert both
/// outcomes of a roll ("when the bite succeeds, ..." / "when it fails,
/// ...") without depending on statistics. Panics on an unscripted call or
/// a type mismatch (asking `chance` when a `pick_index` was queued, or
/// vice versa) — a test that under- or mis-scripts its answers has a bug
/// worth surfacing loudly, not silently falling back to some default.
#[cfg(test)]
pub struct ScriptedRngSource {
    answers: std::collections::VecDeque<ScriptedAnswer>,
}

#[cfg(test)]
enum ScriptedAnswer {
    Chance(bool),
    Index(usize),
}

#[cfg(test)]
impl ScriptedRngSource {
    pub fn new() -> Self {
        Self { answers: std::collections::VecDeque::new() }
    }

    pub fn with_chance(mut self, answer: bool) -> Self {
        self.answers.push_back(ScriptedAnswer::Chance(answer));
        self
    }

    pub fn with_index(mut self, answer: usize) -> Self {
        self.answers.push_back(ScriptedAnswer::Index(answer));
        self
    }
}

#[cfg(test)]
impl Default for ScriptedRngSource {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl RngSource for ScriptedRngSource {
    fn chance(&mut self, _percent_chance: u32) -> bool {
        match self.answers.pop_front().expect("ScriptedRngSource ran out of scripted answers") {
            ScriptedAnswer::Chance(answer) => answer,
            ScriptedAnswer::Index(_) => panic!("ScriptedRngSource: expected a chance() call, but an index() was scripted next"),
        }
    }

    fn pick_index(&mut self, len: usize) -> usize {
        assert!(len > 0, "pick_index called with no candidates to pick from");
        match self.answers.pop_front().expect("ScriptedRngSource ran out of scripted answers") {
            ScriptedAnswer::Index(answer) => {
                assert!(answer < len, "scripted index {answer} out of bounds for len {len}");
                answer
            }
            ScriptedAnswer::Chance(_) => panic!("ScriptedRngSource: expected a pick_index() call, but a chance() was scripted next"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_percent_chance_always_fails() {
        let mut rng = SeededRngSource::new(1);
        for _ in 0..100 {
            assert!(!rng.chance(0));
        }
    }

    #[test]
    fn a_hundred_percent_chance_always_succeeds() {
        let mut rng = SeededRngSource::new(1);
        for _ in 0..100 {
            assert!(rng.chance(100));
        }
    }

    #[test]
    fn a_chance_above_a_hundred_is_clamped_to_always_succeed() {
        let mut rng = SeededRngSource::new(1);
        for _ in 0..100 {
            assert!(rng.chance(150));
        }
    }

    #[test]
    fn pick_index_stays_in_bounds() {
        let mut rng = SeededRngSource::new(2);
        for _ in 0..200 {
            assert!(rng.pick_index(3) < 3);
        }
    }

    #[test]
    fn scripted_rng_returns_answers_in_order() {
        let mut rng = ScriptedRngSource::new().with_chance(true).with_chance(false).with_index(2);
        assert!(rng.chance(50));
        assert!(!rng.chance(50));
        assert_eq!(rng.pick_index(5), 2);
    }

    #[test]
    #[should_panic(expected = "ran out of scripted answers")]
    fn scripted_rng_panics_when_exhausted() {
        let mut rng = ScriptedRngSource::new();
        rng.chance(50);
    }
}
