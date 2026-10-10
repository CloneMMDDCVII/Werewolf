//! Building a batch of questions, sending it, and reading the answers back.

use super::{Answer, OnAnswer, Phase, Presenter, Prompt, Question};
use crate::roles::PlayerId;

/// Sends one phase's questions and returns validated answers, one per
/// question (see `validate_pick`); an answer of the wrong shape, or a
/// missing one, counts as no answer.
pub(super) async fn ask_phase(
    presenter: &mut dyn Presenter,
    phase: Phase,
    questions: &[Question],
    on_answer: &mut dyn OnAnswer,
) -> Vec<Answer> {
    if questions.is_empty() {
        return vec![];
    }
    let mut answers = presenter
        .ask_batch(phase, questions.to_vec(), on_answer)
        .await
        .into_iter();
    questions.iter().map(|q| validate_answer(q, answers.next())).collect()
}

fn validate_answer(question: &Question, answer: Option<Answer>) -> Answer {
    match (question, answer) {
        (Question::Targets { options, count, .. }, Some(Answer::Targets(Some(picked)))) => {
            Answer::Targets(validate_pick(picked, options, *count))
        }
        (Question::Targets { .. }, _) => Answer::Targets(None),
        (Question::Toggle { .. }, Some(Answer::Toggle(b))) => Answer::Toggle(b),
        (Question::Toggle { .. }, _) => Answer::Toggle(false),
    }
}

/// What counts as a well-formed pick: exactly `count` entries, all
/// distinct, all among the options offered. An answer outside the options
/// (a stale button for a player who has since died, say) counts as no
/// answer, never as a pick.
pub(super) fn validate_pick(picked: Vec<PlayerId>, options: &[PlayerId], count: usize) -> Option<Vec<PlayerId>> {
    let all_distinct = (1..picked.len()).all(|i| !picked[..i].contains(&picked[i]));
    let all_offered = picked.iter().all(|p| options.contains(p));
    (picked.len() == count && all_distinct && all_offered).then_some(picked)
}

/// Builds a batch while remembering where each question landed.
#[derive(Default)]
pub(super) struct BatchBuilder {
    pub(super) questions: Vec<Question>,
}

impl BatchBuilder {
    pub(super) fn targets(&mut self, player: PlayerId, prompt: Prompt, options: &[PlayerId], count: usize) -> usize {
        self.questions.push(Question::Targets {
            player,
            prompt,
            options: options.to_vec(),
            count,
            text: None,
        });
        self.questions.len() - 1
    }

    pub(super) fn targets_with_text(
        &mut self,
        player: PlayerId,
        prompt: Prompt,
        options: &[PlayerId],
        text: crate::messages::Message,
    ) -> usize {
        self.questions.push(Question::Targets {
            player,
            prompt,
            options: options.to_vec(),
            count: 1,
            text: Some(text),
        });
        self.questions.len() - 1
    }

    pub(super) fn toggle(&mut self, player: PlayerId, prompt: Prompt) -> usize {
        self.questions.push(Question::Toggle { player, prompt });
        self.questions.len() - 1
    }
}

pub(super) fn targets_answer(answers: &[Answer], index: Option<usize>) -> Option<Vec<PlayerId>> {
    match index.map(|i| &answers[i]) {
        Some(Answer::Targets(t)) => t.clone(),
        _ => None,
    }
}

pub(super) fn single_target_answer(answers: &[Answer], index: Option<usize>) -> Option<PlayerId> {
    targets_answer(answers, index).and_then(|v| v.into_iter().next())
}

pub(super) fn toggle_answer(answers: &[Answer], index: Option<usize>) -> bool {
    matches!(index.map(|i| &answers[i]), Some(Answer::Toggle(true)))
}
