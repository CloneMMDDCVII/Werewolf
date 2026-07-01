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
use game_engine::orchestrator::{
    death_locale_key, game_over_locale_key, prompt_locale_key, transform_locale_key, NarrationEvent,
    Presenter, Prompt,
};
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
        Prompt::BlacksmithSilver => "(no locale key in English.xml) Who do you want to protect with silver?",
        Prompt::SpumpkinDetonate => "(no locale key in English.xml) Who do you want to detonate on?",
        _ => "Who do you choose?",
    }
}

/// Same idea as `prompt_fallback_text`, for deaths with no exact legacy
/// flavor key (see `orchestrator::death_locale_key` — today that's every
/// `KillMethod` except `Lynch`, since the legacy per-role `*Killed` keys
/// are all Serial-Killer-specific and not yet modeled here).
fn death_fallback_template() -> &'static str {
    "{0} did not survive."
}

/// Same idea, for a transform with no legacy key (see
/// `orchestrator::transform_locale_key` — e.g. a Doppelganger copying a
/// role other than the four with dedicated flavor text).
fn transform_fallback_template() -> &'static str {
    "{0} has transformed."
}

/// Whether a prompt is asked during the night or day phase — purely for
/// transcript labeling, mirroring which of `resolve_night`/`resolve_day`
/// asks it (see `orchestrator::one_target_day_prompt`/`toggle_day_prompt`).
fn phase_label(prompt: Prompt) -> &'static str {
    match prompt {
        Prompt::GunnerShoot
        | Prompt::BlacksmithSilver
        | Prompt::SpumpkinDetonate
        | Prompt::MayorReveal
        | Prompt::PacifistPeace
        | Prompt::TroublemakerTrouble
        | Prompt::LynchVote => "Day",
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
    pub fn new(
        inner: &'a mut dyn Presenter,
        pack: &'a LanguagePack,
        names: &'a HashMap<PlayerId, String>,
    ) -> Self {
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
        strip_unfilled_placeholders(&raw)
    }

    pub fn push_line(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }
}

/// Some locale templates carry a `{0}`/`{1}` for data this proof-of-concept
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
            // Matches the real bot's per-voter announcement
            // (`PlayerVotedLynch`, "{0} has voted to lynch {1}.") rather
            // than the generic "asked/chose" phrasing below - lynch votes
            // are cast in public, not asked privately.
            if let Some(targets) = &answer {
                let template = self
                    .pack
                    .get("PlayerVotedLynch")
                    .unwrap_or("{0} has voted to lynch {1}.");
                let line = template
                    .replacen("{0}", &self.name(player), 1)
                    .replacen("{1}", &self.name(targets[0]), 1);
                self.lines.push(line);
            }
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
            NarrationEvent::Death { victim, role, method } => {
                let name = self.name(victim);
                let template = match death_locale_key(role, method) {
                    Some(key) => self.pack.get(key).unwrap_or_else(|| death_fallback_template()),
                    None => self
                        .pack
                        .get("GenericDeathNoReveal")
                        .unwrap_or_else(|| death_fallback_template()),
                };
                // `.replace`, not `.replacen(..., 1)`: `GenericDeathNoReveal`
                // repeats `{0}` twice in the same sentence (the victim's
                // name appears once as "notices that {0} is not around"
                // and again as "the remains of {0}") - replacing only the
                // first occurrence left the second one to `strip_unfilled_
                // placeholders`, rendering as a bogus "[?]" instead of the
                // name.
                let line = template.replace("{0}", &name).replacen("{1}", "", 1);
                self.lines.push(format!("{} ({method:?})", strip_unfilled_placeholders(&line)));
            }
            NarrationEvent::Transform { player, from, to } => {
                let name = self.name(player);
                let template = match transform_locale_key(from, to) {
                    Some(key) => self.pack.get(key).unwrap_or_else(|| transform_fallback_template()),
                    None => transform_fallback_template(),
                };
                // `{0}` means different things across these legacy keys -
                // the transforming player's own name in `DGToWolf`, but
                // the *dead role model's* name in `WildChildTransform`/
                // `ApprenticeNowSeer`. `NarrationEvent::Transform` doesn't
                // carry "who died to cause this," only who transformed
                // and between which roles, so this always fills `{0}`
                // with the transforming player's own name - correct for
                // the Doppelganger keys, an honest approximation for the
                // other two rather than plumbing more context through
                // for this slice.
                let line = template.replace("{0}", &name);
                self.lines
                    .push(format!("{} ({from:?} -> {to:?})", strip_unfilled_placeholders(&line)));
            }
            NarrationEvent::GameOver { winner } => {
                let template = match game_over_locale_key(&winner) {
                    Some(key) => self.pack.get(key).unwrap_or("The game is over."),
                    None => "The game is over.",
                };
                self.lines.push(format!("{template} ({winner:?})"));
            }
        }
    }
}
