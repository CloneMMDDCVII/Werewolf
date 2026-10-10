//! Everything that asks players something, and the day. Questions go out
//! through the [`Presenter`] trait, so this module never knows whether it
//! is talking to Telegram or a scripted test. `bot` and `sim` each
//! implement `Presenter` their own way.
//!
//! `Presenter` asks for targets with a count (`ask_targets(.., count)`)
//! rather than one method per number of targets, so a role that picks
//! three players needs no new method on every presenter.
//!
//! Which question each role gets is its `RoleDef` (`roles::def`). The
//! night is asked in one batch, then a second for roles that need the
//! wolves' target first (`NightQuestion::AfterWolves`, the Witch).
//!
//! - [`presenter`]: the `Presenter` trait, `Question`, `Answer`,
//!   `OnAnswer`, `Phase` and `NarrationEvent`;
//! - [`prompt`]: the `Prompt` enum and its legacy locale keys;
//! - [`locale`]: the locale keys for deaths and game endings;
//! - [`night_ask`]: asking the night (`resolve_night_with_freeze`);
//! - [`day`]: the day's questions, the lynch tally and its results;
//! - [`deaths`]: the Hunter's shot, lovers, the Spumpkin and the Grave
//!   Digger's trap;
//! - [`batch`]: building, sending and reading back a batch of questions.

mod batch;
pub mod day;
pub mod deaths;
pub mod locale;
pub mod night_ask;
pub mod presenter;
pub mod prompt;

pub use day::{apply_day_results, last_day_veto, resolve_day, resolve_lynch_vote, LynchResult};
pub(crate) use deaths::grave_digger_fall_chance_percent;
pub use deaths::{resolve_hunter_shots, resolve_lover_deaths, resolve_spumpkin_detonation};
pub use locale::{all_mapped_locale_keys, death_locale_key, game_over_locale_key, game_over_message};
pub use night_ask::{resolve_night_with_freeze, TeamVotes};
pub use presenter::{Answer, NarrationEvent, OnAnswer, Phase, Presenter, Question, Quiet};
pub use prompt::{prompt_locale_key, Prompt, ALL_PROMPTS};

use crate::roles::PlayerId;
use shared::Role;

/// What every alive player brings into either phase: who they are and
/// what role they're currently playing. Shared by `resolve_night_with_freeze`
/// and `resolve_day` — a player's identity and role don't change between the
/// two, only which questions get asked.
#[derive(Debug, Clone, Copy)]
pub struct AlivePlayer {
    pub id: PlayerId,
    pub role: Role,
}
