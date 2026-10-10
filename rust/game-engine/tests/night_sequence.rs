//! Whole-game checks for features/night-sequence.feature: what the game
//! loop adds around the night resolver.

use async_trait::async_trait;
use game_engine::messages::{Announcement, NightNote, Note};
use game_engine::orchestrator::{AlivePlayer, NarrationEvent, Presenter, Prompt};
use game_engine::rng::SeededRngSource;
use game_engine::roles::PlayerId;
use game_engine::rules::GameRules;
use game_engine::run_game_with_rules;
use shared::{KillMethod, Role, ALL_ROLES};

/// Answers prompts from a fixed table, says no to every toggle, and never
/// votes to lynch.
struct Night {
    answers: Vec<(Prompt, PlayerId)>,
    asked: Vec<(PlayerId, Prompt, u32)>,
    round: u32,
    narrated: Vec<NarrationEvent>,
}

impl Night {
    fn new(answers: &[(Prompt, PlayerId)]) -> Self {
        Night { answers: answers.to_vec(), asked: vec![], round: 1, narrated: vec![] }
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

    fn asked_on(&self, player: PlayerId, prompt: Prompt) -> Vec<u32> {
        self.asked.iter().filter(|(p, pr, _)| *p == player && *pr == prompt).map(|a| a.2).collect()
    }
}

#[async_trait(?Send)]
impl Presenter for Night {
    async fn ask_targets(&mut self, player: PlayerId, prompt: Prompt, options: &[PlayerId], _: usize) -> Option<Vec<PlayerId>> {
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
    roles.iter().enumerate().map(|(i, &role)| AlivePlayer { id: PlayerId(i as u64 + 1), role }).collect()
}

const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);
const P3: PlayerId = PlayerId(3);

async fn play(players: &[AlivePlayer], presenter: &mut Night, seed: u64, rounds: u32) {
    let rules = GameRules::default();
    run_game_with_rules(players, ALL_ROLES, &rules, presenter, &mut SeededRngSource::new(seed), rounds).await;
}

#[tokio::test]
async fn a_quiet_night_is_announced() {
    let players = roster(&[Role::Wolf, Role::Villager, Role::Villager, Role::Villager]);
    let mut night = Night::new(&[]);
    play(&players, &mut night, 0, 1).await;
    assert!(night.narrated.contains(&NarrationEvent::Announce(Announcement::NoAttack)));
}

#[tokio::test]
async fn the_chemist_brews_on_the_first_night_and_is_asked_on_the_second() {
    let players = roster(&[Role::Chemist, Role::Wolf, Role::Villager, Role::Villager, Role::Villager, Role::Villager]);
    let mut night = Night::new(&[(Prompt::ChemistBrew, P3)]);
    play(&players, &mut night, 0, 2).await;
    assert_eq!(night.asked_on(P1, Prompt::ChemistBrew), vec![2]);
    assert_eq!(night.notes_to(P1)[0], Note::Night(NightNote::ChemistBrewing));
}

#[tokio::test]
async fn wolves_who_ate_the_drunk_skip_the_next_night() {
    let players = roster(&[Role::Wolf, Role::Drunk, Role::Villager, Role::Villager, Role::Villager, Role::Villager]);
    let mut night = Night::new(&[(Prompt::WolfEat, P2)]);
    play(&players, &mut night, 0, 3).await;
    assert_eq!(night.asked_on(P1, Prompt::WolfEat), vec![1, 3]);
    assert_eq!(night.notes_to(P1)[0], Note::Night(NightNote::WolvesEatDrunk(P2)));
}

#[tokio::test]
async fn a_bitten_player_is_told_once_the_night_is_over() {
    let players = roster(&[Role::AlphaWolf, Role::Villager, Role::Villager, Role::Villager, Role::Villager]);
    let mut bitten = 0;
    for seed in 0..30 {
        let mut night = Night::new(&[(Prompt::WolfEat, P2)]);
        play(&players, &mut night, seed, 1).await;
        let told = night.notes_to(P2).contains(&Note::Night(NightNote::PlayerBitten));
        let died = night.narrated.iter().any(|e| matches!(e, NarrationEvent::Death { victim, .. } if *victim == P2));
        assert!(told != died, "seed {seed}: either bitten or eaten");
        bitten += told as u32;
    }
    assert!(bitten > 0 && bitten < 30, "about one bite in five, got {bitten}/30");
}

#[tokio::test]
async fn the_burned_share_one_line_when_roles_are_shown() {
    let players = roster(&[Role::Arsonist, Role::Villager, Role::Villager, Role::Wolf, Role::Villager, Role::Villager]);
    // Douses P2 on night one and P3 on night two, then sparks on night three.
    struct Arsonist {
        inner: Night,
    }
    #[async_trait(?Send)]
    impl Presenter for Arsonist {
        async fn ask_targets(&mut self, player: PlayerId, prompt: Prompt, options: &[PlayerId], n: usize) -> Option<Vec<PlayerId>> {
            if prompt == Prompt::ArsonistDouse {
                let target = if self.inner.round == 1 { P2 } else { P3 };
                return options.contains(&target).then(|| vec![target]);
            }
            self.inner.ask_targets(player, prompt, options, n).await
        }
        async fn ask_toggle(&mut self, _: PlayerId, prompt: Prompt) -> bool {
            prompt == Prompt::ArsonistSpark && self.inner.round == 3
        }
        fn advance_round(&mut self) {
            self.inner.advance_round();
        }
        async fn narrate(&mut self, event: NarrationEvent) {
            self.inner.narrate(event).await;
        }
    }
    let mut arsonist = Arsonist { inner: Night::new(&[]) };
    let rules = GameRules::default();
    run_game_with_rules(&players, ALL_ROLES, &rules, &mut arsonist, &mut SeededRngSource::new(0), 3).await;
    let burns: Vec<_> = arsonist
        .inner
        .narrated
        .iter()
        .filter_map(|e| match e {
            NarrationEvent::Death { victim, method: KillMethod::Burn, message, .. } => Some((*victim, message.as_ref().map(|m| m.key))),
            _ => None,
        })
        .collect();
    assert_eq!(burns, vec![(P2, Some("Burning")), (P3, None)]);
}
