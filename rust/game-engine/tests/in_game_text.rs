//! Whole-game checks for features/in-game-text.feature: day ability lines
//! and the death lines the engine picks.

use async_trait::async_trait;
use game_engine::messages::{Announcement, MessageArg};
use game_engine::orchestrator::{AlivePlayer, NarrationEvent, Presenter, Prompt};
use game_engine::rng::SeededRngSource;
use game_engine::roles::PlayerId;
use game_engine::run_game;
use shared::{KillMethod, Role};

/// Everyone votes for `lynch`; `yes` toggles and `targets` answers are fixed.
struct Day {
    lynch: PlayerId,
    yes: Vec<Prompt>,
    targets: Vec<(Prompt, PlayerId)>,
    narrated: Vec<NarrationEvent>,
}

impl Day {
    fn new(lynch: PlayerId) -> Self {
        Day { lynch, yes: vec![], targets: vec![], narrated: vec![] }
    }

    fn announcements(&self) -> Vec<Announcement> {
        self.narrated
            .iter()
            .filter_map(|e| match e {
                NarrationEvent::Announce(a) => Some(a.clone()),
                _ => None,
            })
            .collect()
    }

    fn death_line(&self, who: PlayerId) -> game_engine::messages::Message {
        self.narrated
            .iter()
            .find_map(|e| match e {
                NarrationEvent::Death { victim, message: Some(m), .. } if *victim == who => Some(m.clone()),
                _ => None,
            })
            .expect("the player died with a line")
    }
}

#[async_trait(?Send)]
impl Presenter for Day {
    async fn ask_targets(&mut self, player: PlayerId, prompt: Prompt, options: &[PlayerId], _: usize) -> Option<Vec<PlayerId>> {
        let pick = match prompt {
            Prompt::LynchVote if player != self.lynch => Some(self.lynch),
            Prompt::LynchVote => options.iter().copied().find(|&o| o != self.lynch),
            _ => self.targets.iter().find(|(p, _)| std::mem::discriminant(p) == std::mem::discriminant(&prompt)).map(|&(_, t)| t),
        }?;
        options.contains(&pick).then(|| vec![pick])
    }

    async fn ask_toggle(&mut self, _: PlayerId, prompt: Prompt) -> bool {
        self.yes.contains(&prompt)
    }

    async fn narrate(&mut self, event: NarrationEvent) {
        self.narrated.push(event);
    }
}

fn roster(roles: &[Role]) -> Vec<AlivePlayer> {
    roles.iter().enumerate().map(|(i, &role)| AlivePlayer { id: PlayerId(i as u64 + 1), role }).collect()
}

const P2: PlayerId = PlayerId(2);
const P3: PlayerId = PlayerId(3);

#[tokio::test]
async fn the_mayor_and_the_pacifist_announce_themselves() {
    let players = roster(&[Role::Wolf, Role::Mayor, Role::Pacifist, Role::Villager, Role::Villager, Role::Villager]);
    let mut day = Day::new(PlayerId(4));
    day.yes = vec![Prompt::MayorReveal, Prompt::PacifistPeace];
    run_game(&players, &mut day, &mut SeededRngSource::new(0), 1).await;
    let said = day.announcements();
    assert!(said.contains(&Announcement::MayorReveal { mayor: P2 }));
    assert!(said.contains(&Announcement::PacifistNoLynch { pacifist: P3 }));
    assert!(said.contains(&Announcement::PacifistNoLynchNow));
    assert!(!day.narrated.iter().any(|e| matches!(e, NarrationEvent::Death { method: KillMethod::Lynch, .. })));
}

#[tokio::test]
async fn the_prince_survives_their_first_lynch_and_says_so() {
    let players = roster(&[Role::Wolf, Role::Prince, Role::Villager, Role::Villager, Role::Villager]);
    let mut day = Day::new(P2);
    run_game(&players, &mut day, &mut SeededRngSource::new(0), 1).await;
    assert!(day.announcements().contains(&Announcement::PrinceLynched { prince: P2 }));
}

#[tokio::test]
async fn a_lynched_hunter_who_shoots_nobody_says_so() {
    let players = roster(&[Role::Wolf, Role::Hunter, Role::Villager, Role::Villager, Role::Villager]);
    let mut day = Day::new(P2);
    run_game(&players, &mut day, &mut SeededRngSource::new(0), 1).await;
    assert!(day.announcements().contains(&Announcement::HunterSkipChoice { hunter: P2, lynched: true }));
}

#[tokio::test]
async fn a_lynched_hunter_who_shoots_names_both() {
    let players = roster(&[Role::Wolf, Role::Hunter, Role::Villager, Role::Villager, Role::Villager]);
    let mut day = Day::new(P2);
    day.targets = vec![(Prompt::HunterFinalShotLynched, P3)];
    run_game(&players, &mut day, &mut SeededRngSource::new(0), 1).await;
    let line = day.death_line(P3);
    assert_eq!(line.key, "HunterKilledFinalLynched");
    assert_eq!(line.args[..2], [MessageArg::Player(P2), MessageArg::Player(P3)]);
}
