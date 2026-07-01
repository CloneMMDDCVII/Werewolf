//! Turns a simulated game into the same kind of text a real Telegram bot
//! would have sent — one line per message, in order — instead of just a
//! pass/fail assertion. `TranscriptPresenter` wraps any other `Presenter`
//! (typically `FixturePresenter`, replaying real history) and, for every
//! question asked, logs the flavor text pulled from the real
//! `Languages/English.xml` locale pack alongside the question and answer.
//!
//! This is deliberately *not* a faithful re-implementation of Werewolf.cs's
//! messaging: a few prompts (Cupid's second target, Blacksmith, Spumpkin,
//! anything about the Witch) have no exact matching locale key, either
//! because there's no locale entry for that menu at all or because the
//! role isn't in the legacy game. Those get an honestly-labeled generic
//! line instead of a fabricated locale-perfect one — see
//! `prompt_fallback_text`. Augur has no `Prompt` at all (see
//! `roles::augur`'s doc: it has no player-facing choice to narrate).
//! Everything else — including Fool/Sorcerer/Oracle, which this file
//! initially mismapped as having no locale key before checking
//! Werewolf.cs:5174-5178 directly — uses the exact real menu text.

use async_trait::async_trait;
use game_engine::orchestrator::{Presenter, Prompt};
use game_engine::roles::PlayerId;
use i18n::LanguagePack;
use shared::KillMethod;
use std::collections::HashMap;

/// Looks up the real flavor text for a prompt, where the legacy game asked
/// one via an exact-matching locale key. `None` means "no such key" (the
/// question is either automatic in the legacy game, or new to this
/// proof-of-concept) — callers fall back to a generic description rather
/// than inventing wording that was never actually shown to a player.
fn prompt_locale_key(prompt: Prompt) -> Option<&'static str> {
    match prompt {
        Prompt::WolfEat => Some("AskEat"),
        // Seer, Fool, Sorcerer, and Oracle are all asked the exact same
        // "Who do you want to see?" menu in the legacy game
        // (Werewolf.cs:5174-5178: one `case` block covers all four) - an
        // earlier version of this map wrongly treated Fool/Sorcerer/Oracle
        // as having no matching prompt at all.
        Prompt::SeerCheck | Prompt::FoolInvestigate | Prompt::SorcererInvestigate | Prompt::OracleInvestigate => {
            Some("AskSee")
        }
        Prompt::HarlotVisit => Some("AskVisit"),
        Prompt::GuardianAngelProtect => Some("AskGuard"),
        Prompt::DetectiveInvestigate => Some("AskDetect"),
        // Werewolf.cs:5216-5219.
        Prompt::CultistHunterInvestigate => Some("AskHunt"),
        Prompt::WildChildRoleModel => Some("AskRoleModel"),
        Prompt::CupidLink => Some("AskCupid1"),
        Prompt::LynchVote => Some("AskLynch"),
        Prompt::GunnerShoot => Some("AskShoot"),
        Prompt::CultistConvert => Some("AskConvert"),
        Prompt::MayorReveal => Some("AskMayor"),
        Prompt::PacifistPeace => Some("AskPacifist"),
        Prompt::SandmanSleep => Some("AskSandman"),
        Prompt::ThiefSteal => Some("AskThief"),
        Prompt::ChemistBrew => Some("AskChemist"),
        Prompt::ArsonistDouse | Prompt::ArsonistSpark => Some("AskArsonist"),
        Prompt::TroublemakerTrouble => Some("AskTroublemaker"),
        // WitchHeal/WitchPoison belong to a role that isn't in the legacy
        // game at all, so there's no locale key to find. BlacksmithSilver
        // and SpumpkinDetonate are menu-driven in Werewolf.cs but have no
        // corresponding key in English.xml (checked directly - not an
        // oversight in this mapping).
        Prompt::WitchHeal | Prompt::WitchPoison | Prompt::BlacksmithSilver | Prompt::SpumpkinDetonate => None,
    }
}

/// Human-readable fallback for a prompt with no locale key (see
/// `prompt_locale_key`), so the transcript still says *something*
/// meaningful rather than an empty string.
fn prompt_fallback_text(prompt: Prompt) -> &'static str {
    match prompt {
        Prompt::WitchHeal => "(new role, not in the legacy game) Who do you want to heal?",
        Prompt::WitchPoison => "(new role, not in the legacy game) Who do you want to poison?",
        Prompt::BlacksmithSilver => "(no locale key in English.xml) Who do you want to protect with silver?",
        Prompt::SpumpkinDetonate => "(no locale key in English.xml) Who do you want to detonate on?",
        _ => "Who do you choose?",
    }
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
/// asked (and its answer) using real locale flavor text where one exists.
/// Delegates every actual decision to `inner` unchanged — this never
/// influences the game, only narrates it.
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
}

impl<'a> TranscriptPresenter<'a> {
    /// Appends one death announcement, using the real `LynchKill` flavor
    /// text for lynch deaths and the generic `GenericDeathNoReveal` text
    /// (no role reveal) for everything else. This is a simplification —
    /// the legacy game has a distinct flavor key per role for Serial
    /// Killer deaths specifically (`GunnerKilled`, `SeerKilled`, etc.),
    /// which this proof-of-concept doesn't attempt to reproduce.
    pub fn push_death(&mut self, victim: PlayerId, method: KillMethod) {
        let name = self.name(victim);
        let line = match method {
            KillMethod::Lynch => {
                let template = self
                    .pack
                    .get("LynchKill")
                    .unwrap_or("The villagers have cast their votes. {0} is dead. {1}");
                template.replacen("{0}", &name, 1).replacen("{1}", "", 1)
            }
            _ => {
                let template = self
                    .pack
                    .get("GenericDeathNoReveal")
                    .unwrap_or("{0} did not survive the night.");
                template.replace("{0}", &name)
            }
        };
        self.lines.push(format!("{line} ({method:?})"));
    }

    pub fn push_line(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }
}
