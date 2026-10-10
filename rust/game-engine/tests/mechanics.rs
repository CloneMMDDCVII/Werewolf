//! Whole-game checks for the mechanics in scopes/finish-game-mechanics
//! .feature: the Snow Wolf freeze, the Troublemaker's second lynch, lynch
//! announcements, Hunter chains and the Gunner's bullet count.

use async_trait::async_trait;
use game_engine::messages::{Announcement, FrozenNotice, Note};
use game_engine::orchestrator::{AlivePlayer, NarrationEvent, Presenter, Prompt};
use game_engine::rng::SeededRngSource;
use game_engine::roles::PlayerId;
use game_engine::run_game;
use shared::{KillMethod, Role};

type Answer = Box<dyn FnMut(&Script, PlayerId, Prompt, &[PlayerId]) -> Option<PlayerId>>;

/// What the scripted players have seen so far, so an answer can depend on
/// it (for example, voting differently in the second lynch).
#[derive(Default)]
struct Script {
    second_lynch: bool,
    round: u32,
}

struct ScriptedPresenter {
    script: Script,
    answer: Answer,
    toggles: Vec<(PlayerId, Prompt)>,
    asked: Vec<(PlayerId, Prompt, Vec<PlayerId>, u32)>,
    narrated: Vec<NarrationEvent>,
}

impl ScriptedPresenter {
    fn new(answer: impl FnMut(&Script, PlayerId, Prompt, &[PlayerId]) -> Option<PlayerId> + 'static) -> Self {
        ScriptedPresenter {
            script: Script { second_lynch: false, round: 1 },
            answer: Box::new(answer),
            toggles: vec![],
            asked: vec![],
            narrated: vec![],
        }
    }

    fn says_yes_to(mut self, player: PlayerId, prompt: Prompt) -> Self {
        self.toggles.push((player, prompt));
        self
    }

    fn private_notes_to(&self, to: PlayerId) -> Vec<Note> {
        self.narrated
            .iter()
            .filter_map(|e| match e {
                NarrationEvent::Private { to: t, note } if *t == to => Some(*note),
                _ => None,
            })
            .collect()
    }

    fn announcements(&self) -> Vec<Announcement> {
        self.narrated
            .iter()
            .filter_map(|e| match e {
                NarrationEvent::Announce(a) => Some(*a),
                _ => None,
            })
            .collect()
    }
}

#[async_trait(?Send)]
impl Presenter for ScriptedPresenter {
    async fn ask_targets(
        &mut self,
        player: PlayerId,
        prompt: Prompt,
        options: &[PlayerId],
        count: usize,
    ) -> Option<Vec<PlayerId>> {
        self.asked.push((player, prompt, options.to_vec(), self.script.round));
        if count != 1 {
            return None;
        }
        (self.answer)(&self.script, player, prompt, options).map(|t| vec![t])
    }

    async fn ask_toggle(&mut self, player: PlayerId, prompt: Prompt) -> bool {
        self.toggles.contains(&(player, prompt))
    }

    fn advance_round(&mut self) {
        self.script.round += 1;
        self.script.second_lynch = false;
    }

    async fn narrate(&mut self, event: NarrationEvent) {
        if event == NarrationEvent::Announce(Announcement::TroubleDoubleLynchNow) {
            self.script.second_lynch = true;
        }
        self.narrated.push(event);
    }
}

fn roster(roles: &[Role]) -> Vec<AlivePlayer> {
    roles
        .iter()
        .enumerate()
        .map(|(i, &role)| AlivePlayer { id: PlayerId(i as u64 + 1), role })
        .collect()
}

const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);
const P3: PlayerId = PlayerId(3);
const P4: PlayerId = PlayerId(4);
const P5: PlayerId = PlayerId(5);

#[tokio::test]
async fn the_troublemaker_causes_a_second_lynch_the_same_day() {
    let players = roster(&[Role::Wolf, Role::Troublemaker, Role::Villager, Role::Villager, Role::Villager, Role::Villager]);
    let mut presenter = ScriptedPresenter::new(|script, voter, prompt, _| match prompt {
        Prompt::LynchVote if !script.second_lynch => Some(if voter == P3 { P4 } else { P3 }),
        Prompt::LynchVote => Some(if voter == P4 { P5 } else { P4 }),
        _ => None,
    })
    .says_yes_to(P2, Prompt::TroublemakerTrouble);

    let outcome = run_game(&players, &mut presenter, &mut SeededRngSource::new(0), 1).await;

    assert_eq!(outcome.deaths, vec![(P3, KillMethod::Lynch), (P4, KillMethod::Lynch)]);
    assert_eq!(
        presenter.announcements(),
        vec![
            Announcement::TroublemakerDoubleLynch { troublemaker: P2 },
            Announcement::TroubleDoubleLynchNow,
        ]
    );
}

#[tokio::test]
async fn a_pacifist_acting_after_the_troublemaker_cancels_both_lynches() {
    let players = roster(&[Role::Wolf, Role::Troublemaker, Role::Pacifist, Role::Villager, Role::Villager]);
    let mut presenter = ScriptedPresenter::new(|_, _, prompt, _| match prompt {
        Prompt::LynchVote => Some(P4),
        _ => None,
    })
    .says_yes_to(P2, Prompt::TroublemakerTrouble)
    .says_yes_to(P3, Prompt::PacifistPeace);

    let outcome = run_game(&players, &mut presenter, &mut SeededRngSource::new(0), 1).await;

    assert!(outcome.deaths.is_empty());
    assert!(!presenter.announcements().contains(&Announcement::TroubleDoubleLynchNow));
}

#[tokio::test]
async fn a_tied_lynch_kills_nobody_and_says_so() {
    let players = roster(&[Role::Wolf, Role::Villager, Role::Villager, Role::Villager]);
    let mut presenter = ScriptedPresenter::new(|_, voter, prompt, _| match prompt {
        Prompt::LynchVote => Some(if voter.0 <= 2 { P3 } else { P2 }),
        _ => None,
    });

    let outcome = run_game(&players, &mut presenter, &mut SeededRngSource::new(0), 1).await;

    assert!(outcome.deaths.is_empty());
    assert_eq!(presenter.announcements(), vec![Announcement::LynchTie]);
}

#[tokio::test]
async fn nobody_voting_is_announced() {
    let players = roster(&[Role::Wolf, Role::Villager, Role::Villager, Role::Villager]);
    let mut presenter = ScriptedPresenter::new(|_, _, _, _| None);
    run_game(&players, &mut presenter, &mut SeededRngSource::new(0), 1).await;
    assert_eq!(presenter.announcements(), vec![Announcement::NoLynchVotes]);
}

#[tokio::test]
async fn a_frozen_seer_learns_nothing_and_cannot_be_frozen_twice_in_a_row() {
    let players = roster(&[Role::SnowWolf, Role::Wolf, Role::Seer, Role::Villager, Role::Villager, Role::Villager]);
    let mut presenter = ScriptedPresenter::new(|_, _, prompt, options| match prompt {
        Prompt::SnowWolfFreeze if options.contains(&P3) => Some(P3),
        Prompt::SnowWolfFreeze => options.first().copied(),
        Prompt::SeerCheck => Some(P2),
        _ => None,
    });

    run_game(&players, &mut presenter, &mut SeededRngSource::new(0), 2).await;

    let seer_notes = presenter.private_notes_to(P3);
    assert_eq!(seer_notes[0], Note::Frozen(FrozenNotice::Seeing));
    // Night 2: the Seer is not frozen and reads the wolf.
    assert_eq!(seer_notes[1], Note::SeerSees { target: P2, role: Role::Wolf });
    let freeze_menus: Vec<&Vec<PlayerId>> = presenter
        .asked
        .iter()
        .filter(|(_, prompt, _, _)| *prompt == Prompt::SnowWolfFreeze)
        .map(|(_, _, options, _)| options)
        .collect();
    assert!(freeze_menus[0].contains(&P3));
    assert!(!freeze_menus[1].contains(&P3), "last night's target is left out");
    assert!(!freeze_menus[0].contains(&P2), "wolves can't be frozen");
}

#[tokio::test]
async fn a_hunter_shot_by_a_hunter_shoots_too() {
    let players = roster(&[Role::Hunter, Role::Hunter, Role::Wolf, Role::Villager, Role::Villager, Role::Villager]);
    let mut presenter = ScriptedPresenter::new(|_, player, prompt, _| match prompt {
        Prompt::LynchVote => Some(if player == P1 { P2 } else { P1 }),
        Prompt::HunterFinalShotLynched => Some(P2),
        Prompt::HunterFinalShotKilled if player == P2 => Some(P3),
        _ => None,
    });

    let outcome = run_game(&players, &mut presenter, &mut SeededRngSource::new(0), 1).await;

    assert_eq!(
        outcome.deaths,
        vec![(P1, KillMethod::Lynch), (P2, KillMethod::HunterShot), (P3, KillMethod::HunterShot)]
    );
}

#[tokio::test]
async fn the_gunner_is_told_how_many_bullets_remain_and_stops_when_empty() {
    let players = roster(&[
        Role::Wolf,
        Role::Gunner,
        Role::Villager,
        Role::Villager,
        Role::Villager,
        Role::Villager,
        Role::Villager,
        Role::Villager,
    ]);
    let mut presenter = ScriptedPresenter::new(|script, _, prompt, _| match prompt {
        Prompt::GunnerShoot { .. } => Some(PlayerId(2 + script.round as u64)),
        _ => None,
    });

    run_game(&players, &mut presenter, &mut SeededRngSource::new(0), 3).await;

    let gunner_prompts: Vec<(Prompt, u32)> = presenter
        .asked
        .iter()
        .filter(|(_, prompt, _, _)| matches!(prompt, Prompt::GunnerShoot { .. }))
        .map(|(_, prompt, _, round)| (*prompt, *round))
        .collect();
    assert_eq!(
        gunner_prompts,
        vec![(Prompt::GunnerShoot { bullets: 2 }, 1), (Prompt::GunnerShoot { bullets: 1 }, 2)]
    );
}

#[tokio::test]
async fn the_detective_snoops_by_day() {
    let players = roster(&[Role::Wolf, Role::Detective, Role::Villager, Role::Villager]);
    let mut presenter = ScriptedPresenter::new(|_, _, prompt, _| match prompt {
        Prompt::DetectiveInvestigate => Some(P1),
        _ => None,
    });

    run_game(&players, &mut presenter, &mut SeededRngSource::new(0), 1).await;

    assert!(presenter.private_notes_to(P2).contains(&Note::DetectiveSnoop { target: P1, role: Role::Wolf }));
    let detective_round_prompts: Vec<u32> = presenter
        .asked
        .iter()
        .filter(|(_, prompt, _, _)| *prompt == Prompt::DetectiveInvestigate)
        .map(|(_, _, _, round)| *round)
        .collect();
    assert_eq!(detective_round_prompts, vec![1], "asked once, in the day phase");
}

#[tokio::test]
async fn the_last_snow_wolf_becomes_a_wolf_and_starts_eating() {
    let players = roster(&[Role::SnowWolf, Role::Wolf, Role::Villager, Role::Villager, Role::Villager, Role::Villager]);
    let mut presenter = ScriptedPresenter::new(|_, voter, prompt, _| match prompt {
        Prompt::LynchVote => Some(if voter == P2 { P3 } else { P2 }),
        _ => None,
    });

    run_game(&players, &mut presenter, &mut SeededRngSource::new(0), 2).await;

    assert!(presenter
        .narrated
        .contains(&NarrationEvent::Transform { player: P1, from: Role::SnowWolf, to: Role::Wolf }));
    assert!(presenter.asked.iter().any(|(p, prompt, _, round)| *p == P1 && *prompt == Prompt::WolfEat && *round == 2));
}
