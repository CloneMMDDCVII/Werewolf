//! A real `Presenter` implementation, standing in for a live Telegram chat
//! by answering questions from a historical fixture instead. This is the
//! actual payoff of the orchestrator's `Presenter` boundary: `game-engine`
//! calls `ask_targets` the exact same way whether the answers come from
//! here or from a real bot.
//!
//! **Honest limitation, not a hidden gap**: the SQL export this fixture
//! data comes from has no vote/action table (established when we designed
//! the export) — only final roles, kills (killer/victim/method/day), and
//! the winner. So only decisions that *produced a recorded kill* can be
//! answered from history. Concretely, today that's just the wolves'
//! night-1 eat target (`KillMethod::Eat`, `day == 1`). Every other
//! question (Seer checks, Detective investigates, GuardianAngel protects,
//! Cupid links, Witch's potions) has no historical trace and always
//! answers `None` — a real decline, not a bug, and not silently pretended
//! to be resolved.

use crate::fixture::GameFixture;
use async_trait::async_trait;
use game_engine::orchestrator::{Presenter, Prompt};
use game_engine::roles::PlayerId;
use shared::KillMethod;

pub struct FixturePresenter<'a> {
    fixture: &'a GameFixture,
    /// Which in-game night this presenter is answering for — the
    /// orchestrator resolves one night at a time, so a fixture covering a
    /// multi-day game needs one `FixturePresenter` per night replayed.
    day: u32,
}

impl<'a> FixturePresenter<'a> {
    pub fn new(fixture: &'a GameFixture, day: u32) -> Self {
        FixturePresenter { fixture, day }
    }

    /// The wolves' actual eat target this night, if history recorded one.
    fn historical_wolf_eat_target(&self) -> Option<PlayerId> {
        self.fixture
            .kills
            .iter()
            .find(|k| k.method == KillMethod::Eat && k.day == self.day)
            .map(|k| PlayerId(k.victim_telegram_id as u64))
    }
}

#[async_trait]
impl<'a> Presenter for FixturePresenter<'a> {
    async fn ask_targets(
        &mut self,
        _player: PlayerId,
        prompt: Prompt,
        _options: &[PlayerId],
        count: usize,
    ) -> Option<Vec<PlayerId>> {
        if count != 1 {
            // No historical data reconstructs any multi-target decision
            // (Cupid's link) either - same "no trace, no answer" rule.
            return None;
        }

        match prompt {
            Prompt::WolfEat => self.historical_wolf_eat_target().map(|t| vec![t]),
            // SeerCheck, HarlotVisit, GuardianAngelProtect,
            // DetectiveInvestigate, FoolInvestigate, WildChildRoleModel,
            // WitchHeal, WitchPoison: none of these produce a GameKill row
            // on their own, so there's nothing in the export to answer
            // from. Declining is the honest answer, not a stand-in "yes."
            _ => None,
        }
    }
}
