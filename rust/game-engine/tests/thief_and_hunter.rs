//! Whole-game checks for features/thief-full-and-cultist-hunter.feature.

use async_trait::async_trait;
use game_engine::messages::{FrozenNotice, Note};
use game_engine::orchestrator::{AlivePlayer, NarrationEvent, Presenter, Prompt};
use game_engine::rng::SeededRngSource;
use game_engine::roles::PlayerId;
use game_engine::rules::GameRules;
use game_engine::run_game_with_rules;
use shared::{KillMethod, Role, ALL_ROLES};

/// Answers night prompts from a fixed table and never votes.
struct Night {
    answers: Vec<(Prompt, PlayerId)>,
    asked: Vec<(PlayerId, Prompt, u32)>,
    round: u32,
    narrated: Vec<NarrationEvent>,
}

impl Night {
    fn new(answers: &[(Prompt, PlayerId)]) -> Self {
        Night {
            answers: answers.to_vec(),
            asked: vec![],
            round: 1,
            narrated: vec![],
        }
    }

    fn notes_to(&self, to: PlayerId) -> Vec<Note> {
        self.narrated
            .iter()
            .filter_map(|e| match e {
                NarrationEvent::Private { to: t, note } if *t == to => Some(*note),
                _ => None,
            })
            .collect()
    }
}

#[async_trait(?Send)]
impl Presenter for Night {
    async fn ask_targets(
        &mut self,
        player: PlayerId,
        prompt: Prompt,
        options: &[PlayerId],
        _: usize,
    ) -> Option<Vec<PlayerId>> {
        self.asked.push((player, prompt, self.round));
        let answer = self.answers.iter().find(|(p, _)| *p == prompt).map(|&(_, t)| t)?;
        options.contains(&answer).then(|| vec![answer])
    }

    async fn ask_toggle(&mut self, _: PlayerId, _: Prompt) -> bool {
        false
    }

    fn advance_round(&mut self) {
        self.round += 1;
    }

    async fn narrate(&mut self, event: NarrationEvent) {
        self.narrated.push(event);
    }
}

fn roster(roles: &[Role]) -> Vec<AlivePlayer> {
    roles
        .iter()
        .enumerate()
        .map(|(i, &role)| AlivePlayer {
            id: PlayerId(i as u64 + 1),
            role,
        })
        .collect()
}

const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);
const P3: PlayerId = PlayerId(3);

fn full() -> GameRules {
    GameRules {
        thief_full: true,
        ..GameRules::default()
    }
}

async fn play(players: &[AlivePlayer], rules: GameRules, presenter: &mut Night, seed: u64, rounds: u32) {
    run_game_with_rules(
        players,
        ALL_ROLES,
        &rules,
        presenter,
        &mut SeededRngSource::new(seed),
        rounds,
    )
    .await;
}

#[tokio::test]
async fn a_full_mode_thief_tries_every_night_and_never_robs_a_wolf() {
    let players = roster(&[Role::Thief, Role::Wolf, Role::Villager, Role::Villager, Role::Villager]);
    let mut night = Night::new(&[(Prompt::ThiefSteal, P2)]);
    play(&players, full(), &mut night, 0, 2).await;
    let thief_asked: Vec<u32> = night
        .asked
        .iter()
        .filter(|(p, pr, _)| *p == P1 && *pr == Prompt::ThiefSteal)
        .map(|a| a.2)
        .collect();
    assert_eq!(thief_asked, vec![1, 2]);
    assert_eq!(night.notes_to(P1), vec![Note::ThiefStealFailed { target: P2 }; 2]);
}

#[tokio::test]
async fn a_landed_steal_makes_the_victim_the_new_thief() {
    let players = roster(&[Role::Thief, Role::Wolf, Role::Seer, Role::Villager, Role::Villager]);
    let mut landed = 0;
    for seed in 0..20 {
        let mut night = Night::new(&[(Prompt::ThiefSteal, P3)]);
        play(&players, full(), &mut night, seed, 1).await;
        let notes = night.notes_to(P1);
        if notes.contains(&Note::ThiefStoleRole { target: P3 }) {
            landed += 1;
            assert_eq!(night.notes_to(P3).last(), Some(&Note::ThiefStoleYourRoleThief));
            assert!(night.narrated.contains(&NarrationEvent::Transform {
                player: P3,
                from: Role::Seer,
                to: Role::Thief
            }));
            assert!(night.narrated.contains(&NarrationEvent::Transform {
                player: P1,
                from: Role::Thief,
                to: Role::Seer
            }));
        } else {
            assert!(
                notes.contains(&Note::ThiefStealFailed { target: P3 }),
                "seed {seed}: {notes:?}"
            );
        }
    }
    assert!(landed > 0 && landed < 20, "about half the steals land, got {landed}/20");
}

#[tokio::test]
async fn a_frozen_full_mode_thief_steals_nothing() {
    let players = roster(&[Role::Thief, Role::SnowWolf, Role::Seer, Role::Villager, Role::Villager]);
    let mut night = Night::new(&[(Prompt::ThiefSteal, P3), (Prompt::SnowWolfFreeze, P1)]);
    play(&players, full(), &mut night, 0, 1).await;
    assert_eq!(night.notes_to(P1), vec![Note::Frozen(FrozenNotice::Thief)]);
}

#[tokio::test]
async fn the_plain_thief_still_steals_on_night_one_and_leaves_a_villager() {
    let players = roster(&[Role::Thief, Role::Wolf, Role::Seer, Role::Villager, Role::Villager]);
    let mut night = Night::new(&[(Prompt::ThiefSteal, P3)]);
    play(&players, GameRules::default(), &mut night, 0, 1).await;
    assert_eq!(night.notes_to(P1), vec![Note::ThiefStoleRole { target: P3 }]);
    assert_eq!(night.notes_to(P3), vec![Note::ThiefStoleYourRoleVillager]);
}

#[tokio::test]
async fn the_cultist_hunter_kills_a_cultist_before_the_cult_converts() {
    let players = roster(&[
        Role::CultistHunter,
        Role::Cultist,
        Role::Wolf,
        Role::Villager,
        Role::Villager,
        Role::Villager,
    ]);
    let mut night = Night::new(&[
        (Prompt::CultistHunterInvestigate, P2),
        (Prompt::CultistConvert, PlayerId(4)),
    ]);
    play(&players, GameRules::default(), &mut night, 0, 1).await;
    assert_eq!(night.notes_to(P1), vec![Note::HunterFindCultist { target: P2 }]);
    assert_eq!(night.notes_to(P2), vec![Note::CHHuntedYou]);
    assert!(night
        .narrated
        .iter()
        .any(|e| matches!(e, NarrationEvent::Death { victim, method: KillMethod::Hunt, .. } if *victim == P2)));
    assert!(!night
        .narrated
        .iter()
        .any(|e| matches!(e, NarrationEvent::Transform { to: Role::Cultist, .. })));
}

#[tokio::test]
async fn the_cultist_hunter_learns_a_target_is_not_a_cultist() {
    let players = roster(&[
        Role::CultistHunter,
        Role::Seer,
        Role::Wolf,
        Role::Villager,
        Role::Villager,
    ]);
    let mut night = Night::new(&[(Prompt::CultistHunterInvestigate, P2)]);
    play(&players, GameRules::default(), &mut night, 0, 1).await;
    assert_eq!(night.notes_to(P1), vec![Note::HunterFailedToFind { target: P2 }]);
}
