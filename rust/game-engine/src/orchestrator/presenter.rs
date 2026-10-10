//! The [`Presenter`] trait, the seam between the engine and whatever
//! asks players and tells them things, with the batch types it works in.

use super::Prompt;
use crate::messages::{Announcement, Note};
use crate::roles::PlayerId;
use async_trait::async_trait;
use shared::{KillMethod, Role};

/// Something that happened, worth narrating, that *isn't* a question —
/// the counterpart to `Prompt` for events instead of decisions. Carries
/// whatever a presenter needs to pick the right legacy flavor text
/// itself (the victim's role *at the moment they died*, not just their
/// id) — a presenter should never need to reach back into game state to
/// answer "what do I say," the event handed to it should already be
/// self-contained, the same way `NightContext`/`DayContext` hand a role
/// everything it needs instead of letting it query other players' state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NarrationEvent {
    Death {
        victim: PlayerId,
        role: Role,
        method: KillMethod,
        /// Whether the group shows roles on death (`ShowRolesDeath`). The
        /// role is always carried; this says whether to say it.
        reveal_role: bool,
        /// The public line for this death (see `death_text`), or `None`
        /// when another line already announced it.
        message: Option<crate::messages::Message>,
    },
    /// A role transform resolved (Werewolf.cs's `Transform(...)` calls) —
    /// Traitor becoming a Wolf, Doppelganger copying a dead role model,
    /// etc. `from`/`to` rather than just `to` because several legacy
    /// messages (`DGToWolf`, `DGToCult`, ...) are phrased around what the
    /// player *was*, not just what they became.
    Transform { player: PlayerId, from: Role, to: Role },
    /// The game has ended. Carries the whole `WinOutcome`, not just a
    /// `Team`: a game that hit the round cap ends with `Unimplemented`,
    /// and an admin's `/killgame` with `Killed`.
    GameOver {
        winner: crate::WinOutcome,
        /// The winner's line (see `game_over_message`).
        message: Option<crate::messages::Message>,
    },
    /// Something told to one player only — see `messages::Note`.
    Private { to: PlayerId, note: Note },
    /// A whole message told to one player only, for what `Note` can't
    /// carry: role info and team lists (see `role_info`).
    Tell {
        to: PlayerId,
        message: crate::messages::Message,
    },
    /// Something told to the whole group — see `messages::Announcement`.
    Announce(Announcement),
    /// The end-of-game summary, right after `GameOver`.
    GameSummary(crate::summary::GameSummary),
}

/// The seam: real I/O (or a test double) lives entirely behind this trait.
/// Async because a real Telegram presenter is fundamentally waiting on
/// network events (a callback query arriving), not something that can be
/// answered synchronously. `?Send`: everything in this crate runs on a
/// single-threaded executor (current-thread `tokio` in tests, and a real
/// bot has no need to hand a `dyn Presenter` across threads), so requiring
/// `Send` futures — `async_trait`'s default — is a constraint nothing
/// here actually needs, and it broke the moment `ask_toggle` got a
/// default body.
#[async_trait(?Send)]
pub trait Presenter {
    /// Ask `player` to pick `count` distinct players from `options` for
    /// the given `prompt`. Returns `None` if the player declines/times out
    /// (a legitimate answer, e.g. Harlot choosing to stay home) — an
    /// implementer that returns `Some(v)` should return exactly `count`
    /// entries; the orchestrator treats anything else as "no answer"
    /// rather than trusting a malformed response. `Some` of an empty list
    /// means the player pressed Skip (see `Answer::Targets`).
    async fn ask_targets(
        &mut self,
        player: PlayerId,
        prompt: Prompt,
        options: &[PlayerId],
        count: usize,
    ) -> Option<Vec<PlayerId>>;

    /// A genuinely different question shape from `ask_targets`: yes/no,
    /// no player selection at all (Sandman's "sleep everyone?", Mayor's
    /// "reveal?", Pacifist's "veto the lynch?"). Kept as its own method
    /// rather than shoehorned into `ask_targets` with `count: 0` — that
    /// would be reusing one method for two different question shapes,
    /// the mirror-image mistake of collapsing `ask_target`/
    /// `ask_two_targets` into one arity-parameterized method. Default
    /// `false`: most presenters (and most roles) never need this.
    async fn ask_toggle(&mut self, _player: PlayerId, _prompt: Prompt) -> bool {
        false
    }

    /// Called by `run_game` between rounds (after both night and day have
    /// resolved with no winner yet). Default no-op — a real Telegram
    /// presenter naturally knows what round it's in from its own state,
    /// it doesn't need telling. `sim::FixturePresenter` is the one
    /// implementer that overrides this, to advance which historical day
    /// it answers questions from.
    fn advance_round(&mut self) {}

    /// Tells the presenter something happened, worth narrating, that
    /// isn't a question — see `NarrationEvent`. Called by `run_game`
    /// *at the moment* each event happens (right after a death is
    /// resolved, before moving on), not reconstructed afterward from
    /// `GameOutcome` — that's what keeps a transcript's death
    /// announcements interleaved with the questions that led to them,
    /// in the actual order they happened, instead of one batch at the
    /// end. Default no-op: most presenters (and every existing one
    /// before this hook existed) have no use for it, same reasoning as
    /// `ask_toggle`'s default.
    async fn narrate(&mut self, _event: NarrationEvent) {}

    /// Players who asked to flee since the last time this was checked
    /// (Werewolf.cs's `RemovePlayer`, the `IsRunning` branch,
    /// 848-860) — a mid-game leave, not the joining-phase one
    /// `control::lobby::Lobby::leave` already covers.
    ///
    /// The legacy game reacts to a flee the instant the command arrives,
    /// mid-question if need be. The engine cannot interrupt an `await`,
    /// so `game::run_game_with_rules` polls this wherever it checks for
    /// the end of the game (after each step of the round). A flee lands
    /// at the next checkpoint, not instantly. A Telegram-backed presenter
    /// would drain whatever `/flee` commands arrived since the last
    /// poll. Default: nobody ever flees, so every existing test/fixture
    /// presenter's behavior is unchanged without needing to implement
    /// this.
    fn drain_fled_players(&mut self) -> Vec<PlayerId> {
        vec![]
    }

    /// Has an admin ended this game outright since the last check
    /// (`/killgame`, Werewolf.cs `Kill()`, 5393-5406)? Polled at the same
    /// checkpoints as `drain_fled_players`, for the same reason (no
    /// interrupt-mid-`await` mechanism) — see `game::run_game`, which
    /// treats a `true` here as an immediate `WinOutcome::Killed`, no
    /// further night/day resolution. Default `false`: every existing
    /// test/fixture presenter's behavior is unchanged.
    fn game_was_killed(&mut self) -> bool {
        false
    }

    /// Asks a whole phase's questions at once and returns one answer per
    /// question, in order. The legacy game sends every night prompt
    /// together under one timer (`SendNightActions`) and every lynch vote
    /// together under another, so a real presenter overrides this to send
    /// them all and wait once; asking one player after another would make
    /// a ten-player night ten timers long.
    ///
    /// `on_answer` is the engine's reaction to each answer the moment it
    /// lands, before the rest of the batch is in: a presenter calls it as
    /// each answer arrives and narrates what it returns right away. That
    /// is how a lynch vote is announced when it is cast
    /// (Werewolf.cs:1125-1137), not after everyone has voted.
    ///
    /// The default asks one question at a time through `ask_targets` and
    /// `ask_toggle`, in order, calling `on_answer` after each, so a
    /// presenter that doesn't override it sees exactly the calls it always
    /// did.
    async fn ask_batch(
        &mut self,
        _phase: Phase,
        questions: Vec<Question>,
        on_answer: &mut dyn OnAnswer,
    ) -> Vec<Answer> {
        let mut answers = Vec::with_capacity(questions.len());
        for (index, question) in questions.into_iter().enumerate() {
            if on_answer.closed(index) {
                answers.push(match question {
                    Question::Targets { .. } => Answer::Targets(None),
                    Question::Toggle { .. } => Answer::Toggle(false),
                });
                continue;
            }
            let answer = match question {
                Question::Targets {
                    player,
                    prompt,
                    options,
                    count,
                    ..
                } => Answer::Targets(self.ask_targets(player, prompt, &options, count).await),
                Question::Toggle { player, prompt } => Answer::Toggle(self.ask_toggle(player, prompt).await),
            };
            for event in on_answer.answered(index, &answer) {
                self.narrate(event).await;
            }
            answers.push(answer);
        }
        answers
    }
}

/// Which phase a batch of questions belongs to, so a presenter can pick
/// the right timer: the night's, or the day's (role actions for the whole
/// day, then the lynch vote).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Night,
    Day,
}

/// One question in a batch: the same two shapes as `ask_targets` and
/// `ask_toggle`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Question {
    Targets {
        player: PlayerId,
        prompt: Prompt,
        options: Vec<PlayerId>,
        count: usize,
        /// What to ask, when it's more than the prompt's own key (the
        /// Arsonist's question lists the houses already doused).
        text: Option<crate::messages::Message>,
    },
    Toggle {
        player: PlayerId,
        prompt: Prompt,
    },
}

impl Question {
    pub fn player(&self) -> PlayerId {
        match self {
            Question::Targets { player, .. } | Question::Toggle { player, .. } => *player,
        }
    }

    pub fn prompt(&self) -> Prompt {
        match self {
            Question::Targets { prompt, .. } | Question::Toggle { prompt, .. } => *prompt,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// `None`: timed out or not answered. `Some` of an empty list: the
    /// player pressed Skip, which only the Hunter's last shot tells apart
    /// from no answer; everywhere else it counts as no answer.
    Targets(Option<Vec<PlayerId>>),
    Toggle(bool),
}

/// See `Presenter::ask_batch`.
pub trait OnAnswer {
    /// Answer `index` of the batch just landed; returns what to tell the
    /// group or the player now. Called at most once per question.
    fn answered(&mut self, index: usize, answer: &Answer) -> Vec<NarrationEvent>;

    /// Whether question `index` is no longer wanted: a presenter stops
    /// waiting for it, and once every question it still waits on is
    /// closed, the batch is over. A lynch vote is closed by a Pacifist's
    /// peace (Werewolf.cs:2575-2590, which also edits the open menus to
    /// say so: `LynchPeaceTimeout`). Default: nothing closes early.
    fn closed(&self, _index: usize) -> bool {
        false
    }

    /// The key a presenter edits a closed question's message to.
    fn closed_key(&self, _index: usize) -> &'static str {
        "TimesUp"
    }
}

/// For batches nothing needs to react to as they're answered.
pub struct Quiet;

impl OnAnswer for Quiet {
    fn answered(&mut self, _: usize, _: &Answer) -> Vec<NarrationEvent> {
        vec![]
    }
}
