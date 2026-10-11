//! Turns a simulated game into the same kind of text a real Telegram bot
//! would have sent — one line per message, in order — instead of just a
//! pass/fail assertion. `TranscriptPresenter` wraps another `Presenter`
//! (typically `FixturePresenter`, replaying real history) and logs a line
//! every time it's asked something (`ask_targets`/`ask_toggle`) or told
//! something happened (`narrate`), pulling real flavor text from
//! `Languages/English.xml` via the canonical `Prompt`/`NarrationEvent`
//! lookup tables `game_engine::orchestrator` owns — this file has no
//! mapping of its own to keep in sync with those; it only decides
//! *fallback* wording for the handful of cases with no exact legacy key
//! (Cupid's second target, Blacksmith, Spumpkin, anything about the
//! Witch), which genuinely is a presentation choice, not a fact about
//! what a `Prompt`/`NarrationEvent` means.
//!
//! Death announcements arrive through `narrate` at the moment `run_game`
//! resolves them — interleaved with the questions that led to them, in
//! the order they actually happened — rather than being reconstructed
//! from `GameOutcome` after the whole game ends.

use async_trait::async_trait;
use game_engine::messages::{render, Message};
use game_engine::orchestrator::{prompt_locale_key, NarrationEvent, Presenter, Prompt};
use game_engine::roles::PlayerId;
use i18n::LanguagePack;
use std::collections::HashMap;

/// Human-readable fallback for a prompt with no locale key (see
/// `orchestrator::prompt_locale_key`), so the transcript still says
/// *something* meaningful rather than an empty string. This is the one
/// mapping that legitimately belongs to a specific presenter rather than
/// the engine: *whether* a legacy key exists is a fact, but *how to phrase
/// its absence* is a presentation choice a Telegram presenter might make
/// differently (e.g. silently skip the line instead).
fn prompt_fallback_text(prompt: Prompt) -> &'static str {
    match prompt {
        Prompt::WitchHeal => "(new role, not in the legacy game) Who do you want to heal?",
        Prompt::WitchPoison => "(new role, not in the legacy game) Who do you want to poison?",
        Prompt::SpumpkinDetonate => "(no locale key in English.xml) Who do you want to detonate on?",
        _ => "Who do you choose?",
    }
}

/// Whether a prompt is asked during the night or day phase — purely for
/// transcript labeling, mirroring which of `resolve_night_with_freeze`/`resolve_day`
/// asks it (each role's `RoleDef::night`/`RoleDef::day`).
fn phase_label(prompt: Prompt) -> &'static str {
    match prompt {
        Prompt::GunnerShoot { .. }
        | Prompt::DetectiveInvestigate
        | Prompt::BlacksmithSilver
        | Prompt::SpumpkinDetonate
        | Prompt::MayorReveal
        | Prompt::PacifistPeace
        | Prompt::TroublemakerTrouble
        | Prompt::LynchVote
        // Only ever asked right after a lynch, which only ever happens in
        // the day phase - unlike `HunterFinalShotKilled`, unambiguous.
        | Prompt::HunterFinalShotLynched => "Day",
        _ => "Night",
    }
}

/// Wraps another `Presenter`, logging one transcript line per question
/// asked (and its answer) and per event narrated, using real locale
/// flavor text where one exists. Delegates every actual decision to
/// `inner` unchanged — this never influences the game, only narrates it.
pub struct TranscriptPresenter<'a> {
    inner: &'a mut dyn Presenter,
    pack: &'a LanguagePack,
    names: &'a HashMap<PlayerId, String>,
    day: u32,
    pub lines: Vec<String>,
}

impl<'a> TranscriptPresenter<'a> {
    pub fn new(inner: &'a mut dyn Presenter, pack: &'a LanguagePack, names: &'a HashMap<PlayerId, String>) -> Self {
        TranscriptPresenter {
            inner,
            pack,
            names,
            day: 1,
            lines: vec![format!("=== Day {} ===", 1)],
        }
    }

    fn name(&self, id: PlayerId) -> String {
        self.names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("Player{}", id.0))
    }

    fn question_text(&self, prompt: Prompt) -> String {
        let raw = match prompt_locale_key(prompt) {
            Some(key) => self
                .pack
                .get(key)
                .map(str::to_string)
                .unwrap_or_else(|| prompt_fallback_text(prompt).to_string()),
            None => prompt_fallback_text(prompt).to_string(),
        };
        let raw = match prompt {
            Prompt::GunnerShoot { bullets } => raw.replace("{0}", &bullets.to_string()),
            _ => raw,
        };
        strip_unfilled_placeholders(&raw)
    }

    /// Fills a `Note`/`Announcement` template through
    /// `messages::render`: players by name, roles by their display name.
    fn render_message(&self, message: &Message) -> String {
        let lookup = |key: &str| self.pack.get(key).map(str::to_string);
        let name = |id: PlayerId| self.name(id);
        strip_unfilled_placeholders(&render(message, &lookup, &name))
    }

    pub fn push_line(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }
}

/// Some locale templates carry a `{0}`/`{1}` for data the engine
/// doesn't track (e.g. `AskShoot`'s "{0} bullet(s) remain." needs a
/// Gunner's remaining-bullet count, which `RoleState` doesn't expose to a
/// presenter). Leaving the raw brace in the transcript would read as a
/// broken template rather than an honest omission, so it's replaced with
/// `[?]` instead — visibly a gap, not a fabricated number.
fn strip_unfilled_placeholders(s: &str) -> String {
    s.replace("{0}", "[?]").replace("{1}", "[?]")
}

#[async_trait(?Send)]
impl<'a> Presenter for TranscriptPresenter<'a> {
    async fn ask_targets(
        &mut self,
        player: PlayerId,
        prompt: Prompt,
        options: &[PlayerId],
        count: usize,
    ) -> Option<Vec<PlayerId>> {
        let answer = self.inner.ask_targets(player, prompt, options, count).await;

        if prompt == Prompt::LynchVote {
            // Lynch votes are public; the engine announces each one
            // (`Announcement::PlayerVotedLynch`) or, in a secret lynch,
            // only the count, so there is nothing to log here.
            return answer;
        }

        let question = self.question_text(prompt);
        let phase = phase_label(prompt);
        let line = match &answer {
            Some(targets) => {
                let names: Vec<String> = targets.iter().map(|&t| self.name(t)).collect();
                format!(
                    "[{phase} {}] {} was asked: \"{}\" -> chose {}",
                    self.day,
                    self.name(player),
                    question,
                    names.join(", ")
                )
            }
            None => format!(
                "[{phase} {}] {} was asked: \"{}\" -> declined",
                self.day,
                self.name(player),
                question
            ),
        };
        self.lines.push(line);
        answer
    }

    async fn ask_toggle(&mut self, player: PlayerId, prompt: Prompt) -> bool {
        let answer = self.inner.ask_toggle(player, prompt).await;
        let question = self.question_text(prompt);
        let phase = phase_label(prompt);
        self.lines.push(format!(
            "[{phase} {}] {} was asked: \"{}\" -> {}",
            self.day,
            self.name(player),
            question,
            if answer { "yes" } else { "no" }
        ));
        answer
    }

    fn advance_round(&mut self) {
        self.inner.advance_round();
        self.day += 1;
        self.lines.push(format!("=== Day {} ===", self.day));
    }

    async fn narrate(&mut self, event: NarrationEvent) {
        match event {
            NarrationEvent::Death {
                victim,
                message,
                method,
                ..
            } => {
                // The engine picks the line (`game_engine::death_text`).
                // `None` means another line already covers this death; a
                // bot sends nothing, but the transcript still notes it so
                // every death stays countable.
                let line = match message {
                    Some(message) => self.render_message(&message),
                    None => format!("[{} dies in the line above]", self.name(victim)),
                };
                self.lines.push(format!("{line} ({method:?})"));
            }
            NarrationEvent::Transform { player, from, to } => {
                // What the player is told comes as `Tell`s (see
                // `game_engine::role_info`); this marks the change itself.
                self.lines
                    .push(format!("[{} turns from {from:?} to {to:?}]", self.name(player)));
            }
            NarrationEvent::Tell { to, message } => {
                let text = self.render_message(&message);
                self.lines.push(format!("(to {}) {text}", self.name(to)));
            }
            NarrationEvent::Private { to, note } => {
                let text = self.render_message(&note.message());
                self.lines.push(format!("(to {}) {text}", self.name(to)));
            }
            NarrationEvent::Announce(announcement) => {
                let text = self.render_message(&announcement.message());
                self.lines.push(text);
            }
            NarrationEvent::GameSummary(summary) => {
                let lookup = |key: &str| self.pack.get(key).map(str::to_string);
                let name = |id: PlayerId| self.name(id);
                let text = summary.render(&lookup, &name);
                self.lines.extend(text.lines().map(str::to_string));
            }
            NarrationEvent::GameOver { winner, message } => {
                let text = match message {
                    Some(message) => self.render_message(&message),
                    None => "The game is over.".to_string(),
                };
                self.lines.push(format!("{text} ({winner:?})"));
            }
        }
    }
}
