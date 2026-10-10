//! `Presenter::ask_batch`: each phase's independent questions reach the
//! presenter together, so a real presenter can ask everyone at once under
//! one timer, and lynch votes are announced as they land. (That presenters which don't implement it behave exactly as
//! before is covered by every other test in this crate, none of which
//! implements `ask_batch`.)

use async_trait::async_trait;
use game_engine::messages::Announcement;
use game_engine::orchestrator::{
    resolve_day, resolve_night, resolve_night_with_freeze, AlivePlayer, Answer, LynchResult, NarrationEvent,
    OnAnswer, Phase, Presenter, Prompt, Question,
};
use game_engine::rng::SeededRngSource;
use game_engine::roles::{NightAction, PlayerId, RoleState};
use game_engine::rules::GameRules;
use shared::Role;
use std::collections::HashMap;

const WOLF_A: PlayerId = PlayerId(1);
const WOLF_B: PlayerId = PlayerId(2);
const SEER: PlayerId = PlayerId(3);
const ANGEL: PlayerId = PlayerId(4);
const VILLAGER: PlayerId = PlayerId(5);

/// Records each batch and answers every target question with VILLAGER
/// (or the first two options for a two-target question), every toggle
/// with "no", reporting each answer as it "arrives". Panics if asked
/// anything one question at a time.
#[derive(Default)]
struct BatchRecorder {
    batches: Vec<(Phase, Vec<Question>)>,
    /// Everything said while a batch was still open.
    said_mid_batch: Vec<NarrationEvent>,
}

#[async_trait(?Send)]
impl Presenter for BatchRecorder {
    async fn ask_targets(&mut self, _: PlayerId, prompt: Prompt, _: &[PlayerId], _: usize) -> Option<Vec<PlayerId>> {
        panic!("{prompt:?} was asked on its own instead of in a batch");
    }

    async fn ask_batch(&mut self, phase: Phase, questions: Vec<Question>, on_answer: &mut dyn OnAnswer) -> Vec<Answer> {
        let answers: Vec<Answer> = questions
            .iter()
            .map(|q| match q {
                Question::Targets { count: 1, .. } => Answer::Targets(Some(vec![VILLAGER])),
                Question::Targets { options, count, .. } => Answer::Targets(Some(options[..*count].to_vec())),
                Question::Toggle { .. } => Answer::Toggle(false),
            })
            .collect();
        for (i, a) in answers.iter().enumerate() {
            self.said_mid_batch.extend(on_answer.answered(i, a));
        }
        self.batches.push((phase, questions));
        answers
    }
}

fn players() -> Vec<AlivePlayer> {
    vec![
        AlivePlayer { id: WOLF_A, role: Role::Wolf },
        AlivePlayer { id: WOLF_B, role: Role::Wolf },
        AlivePlayer { id: SEER, role: Role::Seer },
        AlivePlayer { id: ANGEL, role: Role::GuardianAngel },
        AlivePlayer { id: VILLAGER, role: Role::Villager },
    ]
}

#[tokio::test]
async fn a_night_asks_every_independent_role_in_one_batch() {
    let mut presenter = BatchRecorder::default();
    let mut states: HashMap<PlayerId, RoleState> = HashMap::new();

    let (actions, wolf_target) = resolve_night(&players(), &mut states, &mut presenter).await;

    assert_eq!(presenter.batches.len(), 1, "no stage-2 role, so one batch");
    let (phase, questions) = &presenter.batches[0];
    assert_eq!(*phase, Phase::Night);
    let asked: Vec<(PlayerId, Prompt)> = questions.iter().map(|q| (q.player(), q.prompt())).collect();
    assert_eq!(
        asked,
        vec![
            (WOLF_A, Prompt::WolfEat),
            (WOLF_B, Prompt::WolfEat),
            (SEER, Prompt::SeerCheck),
            (ANGEL, Prompt::GuardianAngelProtect),
        ]
    );
    assert_eq!(wolf_target, Some(VILLAGER));
    assert!(actions.contains(&NightAction::EatVote { target: VILLAGER }));
}

#[tokio::test]
async fn the_witch_is_asked_in_a_second_batch_after_the_wolves() {
    let mut roster = players();
    roster.push(AlivePlayer { id: PlayerId(6), role: Role::Witch });
    let mut presenter = BatchRecorder::default();
    let mut states: HashMap<PlayerId, RoleState> = HashMap::new();

    resolve_night(&roster, &mut states, &mut presenter).await;

    assert_eq!(presenter.batches.len(), 2);
    let second: Vec<Prompt> = presenter.batches[1].1.iter().map(Question::prompt).collect();
    assert_eq!(second, vec![Prompt::WitchHeal, Prompt::WitchPoison]);
    assert!(presenter.batches[0].1.iter().all(|q| q.player() != PlayerId(6)));
}

#[tokio::test]
async fn a_day_asks_every_vote_and_day_action_in_one_batch() {
    let mut roster = players();
    roster.push(AlivePlayer { id: PlayerId(6), role: Role::Mayor });
    let mut presenter = BatchRecorder::default();
    let mut states: HashMap<PlayerId, RoleState> = HashMap::new();
    let mut rng = SeededRngSource::new(1);

    let (_, lynch) = resolve_day(&roster, &mut states, &GameRules::default(), &mut presenter, &mut rng).await;

    assert_eq!(presenter.batches.len(), 1);
    let (phase, questions) = &presenter.batches[0];
    assert_eq!(*phase, Phase::Day);
    let votes = questions.iter().filter(|q| q.prompt() == Prompt::LynchVote).count();
    assert_eq!(votes, roster.len());
    assert!(questions.iter().any(|q| q.prompt() == Prompt::MayorReveal));
    assert_eq!(lynch, LynchResult::Lynched(VILLAGER));
    // Each vote was announced as it landed, while the batch was open.
    // VILLAGER can't vote for themselves, so that answer isn't a vote.
    let announced: Vec<PlayerId> = presenter
        .said_mid_batch
        .iter()
        .map(|e| match e {
            NarrationEvent::Announce(Announcement::PlayerVotedLynch { voter, target }) => {
                assert_eq!(*target, VILLAGER);
                *voter
            }
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(announced, vec![WOLF_A, WOLF_B, SEER, ANGEL, PlayerId(6)]);
}

#[tokio::test]
async fn the_snow_wolfs_freeze_goes_out_with_the_night() {
    let mut roster = players();
    roster.push(AlivePlayer { id: PlayerId(6), role: Role::SnowWolf });
    let mut presenter = BatchRecorder::default();
    let mut states: HashMap<PlayerId, RoleState> = HashMap::new();

    let freeze = Some((PlayerId(6), vec![SEER, ANGEL, VILLAGER]));
    let (_, _, frozen) = resolve_night_with_freeze(&roster, &mut states, freeze, &mut presenter, &[], &[]).await;

    assert_eq!(presenter.batches.len(), 1);
    assert_eq!(presenter.batches[0].1[0].prompt(), Prompt::SnowWolfFreeze);
    assert_eq!(frozen, Some((PlayerId(6), VILLAGER)));
}

/// A presenter answering a batch with garbage gets the same treatment as
/// a malformed `ask_targets` answer: it counts as no answer.
struct GarbageBatch;

#[async_trait(?Send)]
impl Presenter for GarbageBatch {
    async fn ask_targets(&mut self, _: PlayerId, _: Prompt, _: &[PlayerId], _: usize) -> Option<Vec<PlayerId>> {
        unreachable!()
    }

    async fn ask_batch(&mut self, _: Phase, questions: Vec<Question>, _: &mut dyn OnAnswer) -> Vec<Answer> {
        // A player who isn't offered, a toggle where a target was asked,
        // and then nothing at all for the rest.
        let mut out = vec![Answer::Targets(Some(vec![PlayerId(99)]))];
        if questions.len() > 1 {
            out.push(Answer::Toggle(true));
        }
        out
    }
}

#[tokio::test]
async fn malformed_or_missing_batch_answers_count_as_no_answer() {
    let mut presenter = GarbageBatch;
    let mut states: HashMap<PlayerId, RoleState> = HashMap::new();
    let (actions, wolf_target) = resolve_night(&players(), &mut states, &mut presenter).await;
    assert_eq!(wolf_target, None);
    assert!(!actions.iter().any(|a| matches!(a, NightAction::EatVote { .. })));
}
