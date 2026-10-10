//! features/day-abilities.feature: day abilities are announced the moment
//! they're used, and a Pacifist's peace stops the vote it cancels. These
//! go through the default `ask_batch`, which asks one question at a time
//! in batch order, so "the moment it's used" is visible as ordering.

use async_trait::async_trait;
use game_engine::messages::Announcement;
use game_engine::orchestrator::{resolve_day, AlivePlayer, LynchResult, NarrationEvent, Presenter, Prompt};
use game_engine::rng::SeededRngSource;
use game_engine::roles::{DayAction, PlayerId, RoleState};
use game_engine::rules::GameRules;
use game_engine::run_game_with_rules;
use shared::{KillMethod, Role, ALL_ROLES};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
enum Log {
    Asked(PlayerId, Prompt),
    Said(NarrationEvent),
}

/// Votes for `target` (or, for `target` themselves, the player before
/// them), says yes to every prompt in `yes`, and logs everything.
struct Day {
    target: PlayerId,
    yes: Vec<Prompt>,
    /// Whom a Gunner shoots, if anyone.
    shoot: Option<PlayerId>,
    log: Vec<Log>,
}

impl Day {
    fn new(target: PlayerId, yes: &[Prompt]) -> Self {
        Day { target, yes: yes.to_vec(), shoot: None, log: vec![] }
    }

    fn position(&self, entry: &Log) -> usize {
        self.log.iter().position(|l| l == entry).unwrap_or_else(|| panic!("{entry:?} not in {:#?}", self.log))
    }

    fn said(&self, announcement: Announcement) -> usize {
        self.log.iter().filter(|l| **l == Log::Said(NarrationEvent::Announce(announcement.clone()))).count()
    }

    fn votes_asked(&self) -> Vec<PlayerId> {
        self.log
            .iter()
            .filter_map(|l| match l {
                Log::Asked(p, Prompt::LynchVote) => Some(*p),
                _ => None,
            })
            .collect()
    }
}

#[async_trait(?Send)]
impl Presenter for Day {
    async fn ask_targets(&mut self, player: PlayerId, prompt: Prompt, options: &[PlayerId], _: usize) -> Option<Vec<PlayerId>> {
        self.log.push(Log::Asked(player, prompt));
        if let Prompt::GunnerShoot { .. } = prompt {
            return self.shoot.map(|s| vec![s]);
        }
        if prompt != Prompt::LynchVote {
            return None;
        }
        let pick = if options.contains(&self.target) { self.target } else { options[0] };
        Some(vec![pick])
    }

    async fn ask_toggle(&mut self, player: PlayerId, prompt: Prompt) -> bool {
        self.log.push(Log::Asked(player, prompt));
        self.yes.contains(&prompt)
    }

    async fn narrate(&mut self, event: NarrationEvent) {
        self.log.push(Log::Said(event));
    }
}

fn roster(roles: &[Role]) -> Vec<AlivePlayer> {
    roles.iter().enumerate().map(|(i, &role)| AlivePlayer { id: PlayerId(i as u64 + 1), role }).collect()
}

const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);
const P3: PlayerId = PlayerId(3);
const P4: PlayerId = PlayerId(4);
const P5: PlayerId = PlayerId(5);

async fn day(players: &[AlivePlayer], presenter: &mut Day) -> (Vec<DayAction>, LynchResult) {
    let mut states: HashMap<PlayerId, RoleState> = HashMap::new();
    resolve_day(players, &mut states, &GameRules::default(), presenter, &mut SeededRngSource::new(0)).await
}

#[tokio::test]
async fn each_ability_is_announced_as_soon_as_it_is_used() {
    let cases = [
        (Role::Mayor, Prompt::MayorReveal, Announcement::MayorReveal { mayor: P1 }),
        (Role::Pacifist, Prompt::PacifistPeace, Announcement::PacifistNoLynch { pacifist: P1 }),
        (Role::Blacksmith, Prompt::BlacksmithSilver, Announcement::BlacksmithSpreadSilver { blacksmith: P1 }),
        (Role::Troublemaker, Prompt::TroublemakerTrouble, Announcement::TroublemakerDoubleLynch { troublemaker: P1 }),
        (Role::Sandman, Prompt::SandmanSleep, Announcement::SandmanSleepAll { sandman: P1 }),
    ];
    for (role, prompt, announcement) in cases {
        let players = roster(&[role, Role::Villager, Role::Villager, Role::Villager, Role::Wolf]);
        let mut presenter = Day::new(P5, &[prompt]);
        day(&players, &mut presenter).await;
        let told = presenter.position(&Log::Said(NarrationEvent::Announce(announcement.clone())));
        assert_eq!(told, presenter.position(&Log::Asked(P1, prompt)) + 1, "{role:?}: told right after answering");
        assert_eq!(presenter.said(announcement), 1, "{role:?}: told once");
    }
}

#[tokio::test]
async fn nothing_is_announced_again_after_the_day() {
    let players = roster(&[Role::Mayor, Role::Villager, Role::Villager, Role::Villager, Role::Wolf]);
    let mut presenter = Day::new(P5, &[Prompt::MayorReveal]);
    run_game_with_rules(&players, ALL_ROLES, &GameRules::default(), &mut presenter, &mut SeededRngSource::new(0), 1)
        .await;
    assert_eq!(presenter.said(Announcement::MayorReveal { mayor: P1 }), 1);
}

#[tokio::test]
async fn a_used_ability_is_not_offered_again() {
    let players = roster(&[Role::Mayor, Role::Villager, Role::Villager, Role::Villager, Role::Villager, Role::Wolf]);
    let mut presenter = Day::new(PlayerId(6), &[Prompt::MayorReveal]);
    let mut states: HashMap<PlayerId, RoleState> = HashMap::new();
    let rules = GameRules::default();
    let mut rng = SeededRngSource::new(0);
    resolve_day(&players, &mut states, &rules, &mut presenter, &mut rng).await;
    resolve_day(&players, &mut states, &rules, &mut presenter, &mut rng).await;
    let offers = presenter.log.iter().filter(|l| **l == Log::Asked(P1, Prompt::MayorReveal)).count();
    assert_eq!(offers, 1);
}

#[tokio::test]
async fn peace_closes_the_votes_still_open() {
    let players = roster(&[Role::Villager, Role::Pacifist, Role::Villager, Role::Villager, Role::Wolf]);
    let mut presenter = Day::new(P5, &[Prompt::PacifistPeace]);
    let (actions, lynch) = day(&players, &mut presenter).await;
    // P1 and P2 voted before the peace; nobody after it is asked.
    assert_eq!(presenter.votes_asked(), vec![P1, P2]);
    assert_eq!(lynch, LynchResult::NoVotes);
    assert!(actions.contains(&DayAction::Pacify));
}

#[tokio::test]
async fn trouble_after_peace_brings_the_vote_back() {
    let players = roster(&[Role::Pacifist, Role::Troublemaker, Role::Villager, Role::Villager, Role::Wolf]);
    let mut presenter = Day::new(P5, &[Prompt::PacifistPeace, Prompt::TroublemakerTrouble]);
    let (actions, lynch) = day(&players, &mut presenter).await;
    // P2's vote came up while the peace stood, so it was closed; the
    // trouble reopens the vote for everyone after.
    assert_eq!(presenter.votes_asked(), vec![P1, P3, P4, P5]);
    assert_eq!(lynch, LynchResult::Lynched(P5));
    assert_eq!(actions.last(), Some(&DayAction::Trouble), "the trouble came last, so it stands");
}

#[tokio::test]
async fn peace_after_trouble_wins_even_when_the_pacifist_is_listed_first() {
    // The Troublemaker (P2) answers before the Pacifist (P3) in batch
    // order, so peace is the later answer.
    let players = roster(&[Role::Villager, Role::Troublemaker, Role::Pacifist, Role::Villager, Role::Wolf]);
    let mut presenter = Day::new(P5, &[Prompt::PacifistPeace, Prompt::TroublemakerTrouble]);
    let (actions, lynch) = day(&players, &mut presenter).await;
    assert_eq!(lynch, LynchResult::NoVotes);
    assert_eq!(actions.last(), Some(&DayAction::Pacify));
}

#[tokio::test]
async fn a_game_with_peace_lynches_nobody_and_says_so() {
    let players = roster(&[Role::Pacifist, Role::Villager, Role::Villager, Role::Villager, Role::Villager, Role::Wolf]);
    let mut presenter = Day::new(PlayerId(6), &[Prompt::PacifistPeace]);
    run_game_with_rules(&players, ALL_ROLES, &GameRules::default(), &mut presenter, &mut SeededRngSource::new(0), 1)
        .await;
    assert_eq!(presenter.said(Announcement::PacifistNoLynchNow), 1);
    let lynched = presenter.log.iter().any(|l| matches!(l, Log::Said(NarrationEvent::Death { method: KillMethod::Lynch, .. })));
    assert!(!lynched);
    assert_eq!(presenter.said(Announcement::NoLynchVotes), 0, "peace isn't a vote that found nobody");
}

/// Like `Day`, but the Gunner shoots `shot`.
fn gunner_day(target: PlayerId, shot: PlayerId) -> Day {
    let mut day = Day::new(target, &[]);
    day.shoot = Some(shot);
    day
}

#[tokio::test]
async fn the_gunners_shot_kills_at_once() {
    // P2, the Gunner, shoots P4 before P3 and P4 have voted.
    let players = roster(&[Role::Villager, Role::Gunner, Role::Villager, Role::Villager, Role::Wolf]);
    let mut presenter = gunner_day(P4, P4);
    let (actions, lynch) = day(&players, &mut presenter).await;
    let shot_at = presenter.position(&Log::Asked(P2, Prompt::GunnerShoot { bullets: 2 }));
    assert!(
        matches!(&presenter.log[shot_at + 1], Log::Said(NarrationEvent::Death { victim, method: KillMethod::Shoot, message: Some(_), .. }) if *victim == P4),
        "the shot is told right away: {:#?}",
        presenter.log
    );
    assert_eq!(presenter.votes_asked(), vec![P1, P2, P3, P5], "the dead don't vote");
    // Everyone voted for P4: the votes cast before the shot are dropped,
    // and those after it don't count.
    assert_eq!(lynch, LynchResult::NoVotes);
    assert!(actions.contains(&DayAction::Shoot { shooter: P2, target: P4 }));
}

#[tokio::test]
async fn a_shot_is_told_once_in_a_whole_game() {
    let players = roster(&[Role::Villager, Role::Gunner, Role::Villager, Role::Villager, Role::Villager, Role::Wolf]);
    let mut presenter = gunner_day(PlayerId(6), P4);
    run_game_with_rules(&players, ALL_ROLES, &GameRules::default(), &mut presenter, &mut SeededRngSource::new(0), 1).await;
    let told = presenter.log.iter().filter(|l| matches!(l, Log::Said(NarrationEvent::Death { victim, .. }) if *victim == P4)).count();
    assert_eq!(told, 1);
}

#[tokio::test]
async fn shooting_the_wise_elder_is_told_right_after_the_shot() {
    let players = roster(&[Role::Villager, Role::Gunner, Role::Villager, Role::WiseElder, Role::Wolf]);
    let mut presenter = gunner_day(P5, P4);
    day(&players, &mut presenter).await;
    let death = presenter.log.iter().position(|l| matches!(l, Log::Said(NarrationEvent::Death { victim, .. }) if *victim == P4)).unwrap();
    assert_eq!(presenter.log[death + 1], Log::Said(NarrationEvent::Announce(Announcement::GunnerShotWiseElder { gunner: P2, elder: P4 })));
}
