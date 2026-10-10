//! Whole-game checks for features/night-sequence.feature: what the game
//! loop adds around the night resolver.

use async_trait::async_trait;
use game_engine::messages::{render, Announcement, Message, MessageArg, NightNote, Note};
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

    fn asked_on(&self, player: PlayerId, prompt: Prompt) -> Vec<u32> {
        self.asked
            .iter()
            .filter(|(p, pr, _)| *p == player && *pr == prompt)
            .map(|a| a.2)
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

async fn play(players: &[AlivePlayer], presenter: &mut Night, seed: u64, rounds: u32) {
    let rules = GameRules::default();
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
async fn a_quiet_night_is_announced() {
    let players = roster(&[Role::Wolf, Role::Villager, Role::Villager, Role::Villager]);
    let mut night = Night::new(&[]);
    play(&players, &mut night, 0, 1).await;
    assert!(night
        .narrated
        .contains(&NarrationEvent::Announce(Announcement::NoAttack)));
}

#[tokio::test]
async fn the_chemist_brews_on_the_first_night_and_is_asked_on_the_second() {
    let players = roster(&[
        Role::Chemist,
        Role::Wolf,
        Role::Villager,
        Role::Villager,
        Role::Villager,
        Role::Villager,
    ]);
    let mut night = Night::new(&[(Prompt::ChemistBrew, P3)]);
    play(&players, &mut night, 0, 2).await;
    assert_eq!(night.asked_on(P1, Prompt::ChemistBrew), vec![2]);
    assert_eq!(night.notes_to(P1)[0], Note::Night(NightNote::ChemistBrewing));
}

#[tokio::test]
async fn wolves_who_ate_the_drunk_skip_the_next_night() {
    let players = roster(&[
        Role::Wolf,
        Role::Drunk,
        Role::Villager,
        Role::Villager,
        Role::Villager,
        Role::Villager,
    ]);
    let mut night = Night::new(&[(Prompt::WolfEat, P2)]);
    play(&players, &mut night, 0, 3).await;
    assert_eq!(night.asked_on(P1, Prompt::WolfEat), vec![1, 3]);
    assert_eq!(night.notes_to(P1)[0], Note::Night(NightNote::WolvesEatDrunk(P2)));
}

#[tokio::test]
async fn a_bitten_player_is_told_once_the_night_is_over() {
    let players = roster(&[
        Role::AlphaWolf,
        Role::Villager,
        Role::Villager,
        Role::Villager,
        Role::Villager,
    ]);
    let mut bitten = 0;
    for seed in 0..30 {
        let mut night = Night::new(&[(Prompt::WolfEat, P2)]);
        play(&players, &mut night, seed, 1).await;
        let told = night.notes_to(P2).contains(&Note::Night(NightNote::PlayerBitten));
        let died = night
            .narrated
            .iter()
            .any(|e| matches!(e, NarrationEvent::Death { victim, .. } if *victim == P2));
        assert!(told != died, "seed {seed}: either bitten or eaten");
        bitten += told as u32;
    }
    assert!(bitten > 0 && bitten < 30, "about one bite in five, got {bitten}/30");
}

#[tokio::test]
async fn the_burned_share_one_line_when_roles_are_shown() {
    let players = roster(&[
        Role::Arsonist,
        Role::Villager,
        Role::Villager,
        Role::Wolf,
        Role::Villager,
        Role::Villager,
    ]);
    // Douses P2 on night one and P3 on night two, then sparks on night three.
    struct Arsonist {
        inner: Night,
    }
    #[async_trait(?Send)]
    impl Presenter for Arsonist {
        async fn ask_targets(
            &mut self,
            player: PlayerId,
            prompt: Prompt,
            options: &[PlayerId],
            n: usize,
        ) -> Option<Vec<PlayerId>> {
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
    run_game_with_rules(
        &players,
        ALL_ROLES,
        &rules,
        &mut arsonist,
        &mut SeededRngSource::new(0),
        3,
    )
    .await;
    let burns: Vec<_> = arsonist
        .inner
        .narrated
        .iter()
        .filter_map(|e| match e {
            NarrationEvent::Death {
                victim,
                method: KillMethod::Burn,
                message,
                ..
            } => Some((*victim, message.as_ref().map(|m| m.key))),
            _ => None,
        })
        .collect();
    assert_eq!(burns, vec![(P2, Some("Burning")), (P3, None)]);
}

#[tokio::test]
async fn the_grave_digger_is_told_whose_graves_to_dig() {
    let players = roster(&[
        Role::GraveDigger,
        Role::Wolf,
        Role::Villager,
        Role::Villager,
        Role::Villager,
        Role::Villager,
    ]);
    let mut night = Night::new(&[(Prompt::WolfEat, P3)]);
    play(&players, &mut night, 0, 2).await;
    let graves: Vec<Message> = night
        .narrated
        .iter()
        .filter_map(|e| match e {
            NarrationEvent::Tell { to, message } if *to == P1 && message.key.starts_with("Dig") => {
                Some(message.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        graves,
        vec![
            Message::new("DigNoGraves", vec![]),
            Message::new("DigGraves", vec![MessageArg::PlayersAnd(vec![P3])])
        ]
    );
}

#[test]
fn graves_are_listed_with_and() {
    let message = Message::new("DigGraves", vec![MessageArg::PlayersAnd(vec![P2, P3])]);
    let lookup = |key: &str| match key {
        "DigGraves" => Some("You dig for {0}.".to_string()),
        "And" => Some("and".to_string()), // the files' " and " may come back trimmed
        _ => None,
    };
    let text = render(&message, &lookup, &|id| format!("P{}", id.0));
    assert_eq!(text, "You dig for P2 and P3.");
}

#[tokio::test]
async fn the_arsonist_is_shown_the_houses_already_doused() {
    use game_engine::orchestrator::{Answer, OnAnswer, Phase, Question};
    /// Douses P2, then P3; records every night question.
    #[derive(Default)]
    struct Douser {
        nights: Vec<Vec<Question>>,
    }
    #[async_trait(?Send)]
    impl Presenter for Douser {
        async fn ask_targets(&mut self, _: PlayerId, _: Prompt, _: &[PlayerId], _: usize) -> Option<Vec<PlayerId>> {
            None
        }
        async fn ask_batch(&mut self, phase: Phase, questions: Vec<Question>, _: &mut dyn OnAnswer) -> Vec<Answer> {
            let night = self.nights.len();
            let answers = questions
                .iter()
                .map(|q| match q {
                    Question::Targets {
                        prompt: Prompt::ArsonistDouse,
                        ..
                    } if phase == Phase::Night => Answer::Targets(Some(vec![if night == 0 { P2 } else { P3 }])),
                    Question::Targets { .. } => Answer::Targets(None),
                    Question::Toggle { .. } => Answer::Toggle(false),
                })
                .collect();
            if phase == Phase::Night {
                self.nights.push(questions);
            }
            answers
        }
    }
    let players = roster(&[
        Role::Arsonist,
        Role::Villager,
        Role::Villager,
        Role::Wolf,
        Role::Villager,
        Role::Villager,
    ]);
    let mut douser = Douser::default();
    let rules = GameRules::default();
    run_game_with_rules(
        &players,
        ALL_ROLES,
        &rules,
        &mut douser,
        &mut SeededRngSource::new(0),
        2,
    )
    .await;
    let arsonist = |night: usize| -> Vec<Question> {
        douser.nights[night]
            .iter()
            .filter(|q| q.player() == P1)
            .cloned()
            .collect()
    };

    let first = arsonist(0);
    assert_eq!(first.len(), 1, "no spark before anything is doused: {first:?}");
    assert!(matches!(&first[0], Question::Targets { text: Some(m), .. } if m.key == "AskArsonistNoDoused"));

    let second = arsonist(1);
    let Question::Targets {
        options,
        text: Some(text),
        ..
    } = &second[0]
    else {
        panic!("{second:?}")
    };
    assert!(!options.contains(&P2), "a doused house isn't offered again");
    assert_eq!(text.key, "AskArsonist");
    assert_eq!(
        text.then.as_deref(),
        Some(&Message::new(
            "AlreadyDousedList",
            vec![MessageArg::PlayersAnd(vec![P2])]
        ))
    );
    assert!(
        second.iter().any(|q| q.prompt() == Prompt::ArsonistSpark),
        "spark is offered once a house is doused"
    );
}

#[tokio::test]
async fn the_seer_reads_before_the_morning_and_not_once_eaten() {
    let players = roster(&[
        Role::Seer,
        Role::Wolf,
        Role::Villager,
        Role::Villager,
        Role::Villager,
        Role::Villager,
    ]);
    let reading =
        |e: &NarrationEvent| matches!(e, NarrationEvent::Private { to, note: Note::SeerSees { .. } } if *to == P1);

    let mut night = Night::new(&[(Prompt::SeerCheck, P2), (Prompt::WolfEat, P3)]);
    play(&players, &mut night, 0, 1).await;
    let read_at = night.narrated.iter().position(reading).expect("the seer read");
    let death_at = night
        .narrated
        .iter()
        .position(|e| matches!(e, NarrationEvent::Death { .. }))
        .expect("P3 died");
    assert!(read_at < death_at, "the reading comes before the death notices");

    let mut night = Night::new(&[(Prompt::SeerCheck, P2), (Prompt::WolfEat, P1)]);
    play(&players, &mut night, 0, 1).await;
    assert!(
        !night.narrated.iter().any(reading),
        "a seer eaten before their turn learns nothing"
    );
}

/// Records every batch, answers each question with `pick`, and runs the
/// batch's `OnAnswer` like a real presenter, keeping what it says.
struct Recorder<
    F: FnMut(game_engine::orchestrator::Phase, u32, &game_engine::orchestrator::Question) -> Option<PlayerId>,
> {
    pick: F,
    round: u32,
    batches: Vec<(
        game_engine::orchestrator::Phase,
        u32,
        Vec<game_engine::orchestrator::Question>,
    )>,
    narrated: Vec<NarrationEvent>,
}

#[async_trait(?Send)]
impl<F: FnMut(game_engine::orchestrator::Phase, u32, &game_engine::orchestrator::Question) -> Option<PlayerId>>
    Presenter for Recorder<F>
{
    async fn ask_targets(&mut self, _: PlayerId, _: Prompt, _: &[PlayerId], _: usize) -> Option<Vec<PlayerId>> {
        None
    }
    async fn ask_batch(
        &mut self,
        phase: game_engine::orchestrator::Phase,
        questions: Vec<game_engine::orchestrator::Question>,
        on_answer: &mut dyn game_engine::orchestrator::OnAnswer,
    ) -> Vec<game_engine::orchestrator::Answer> {
        use game_engine::orchestrator::{Answer, Question};
        let mut answers = vec![];
        for (i, q) in questions.iter().enumerate() {
            let answer = match q {
                Question::Targets { .. } => Answer::Targets((self.pick)(phase, self.round, q).map(|t| vec![t])),
                Question::Toggle { .. } => Answer::Toggle(false),
            };
            self.narrated.extend(on_answer.answered(i, &answer));
            answers.push(answer);
        }
        self.batches.push((phase, self.round, questions));
        answers
    }
    fn advance_round(&mut self) {
        self.round += 1;
    }
    async fn narrate(&mut self, event: NarrationEvent) {
        self.narrated.push(event);
    }
}

fn recorder<
    F: FnMut(game_engine::orchestrator::Phase, u32, &game_engine::orchestrator::Question) -> Option<PlayerId>,
>(
    pick: F,
) -> Recorder<F> {
    Recorder {
        pick,
        round: 1,
        batches: vec![],
        narrated: vec![],
    }
}

#[tokio::test]
async fn night_menus_leave_out_the_player_and_their_team() {
    use game_engine::orchestrator::{prompt_locale_key, Phase, Question};
    let players = roster(&[
        Role::SerialKiller, // P1
        Role::Seer,         // P2
        Role::Wolf,         // P3
        Role::AlphaWolf,    // P4
        Role::SnowWolf,     // P5
        Role::Cultist,      // P6
        Role::Cultist,      // P7
        Role::Cupid,        // P8
        Role::Arsonist,     // P9
        Role::Villager,     // P10
    ]);
    let all: Vec<PlayerId> = players.iter().map(|p| p.id).collect();
    let but = |out: &[u64]| -> Vec<PlayerId> { all.iter().copied().filter(|id| !out.contains(&id.0)).collect() };
    let mut presenter = recorder(|_, _, _| None);
    let rules = GameRules::default();
    run_game_with_rules(
        &players,
        ALL_ROLES,
        &rules,
        &mut presenter,
        &mut SeededRngSource::new(0),
        1,
    )
    .await;
    let night = &presenter
        .batches
        .iter()
        .find(|(phase, _, _)| *phase == Phase::Night)
        .unwrap()
        .2;
    let menu = |player: u64, prompt: Prompt| -> Vec<PlayerId> {
        night
            .iter()
            .find_map(|q| match q {
                Question::Targets {
                    player: p,
                    prompt: pr,
                    options,
                    ..
                } if p.0 == player && *pr == prompt => Some(options.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("P{player} wasn't asked {prompt:?}: {night:#?}"))
    };
    assert_eq!(menu(1, Prompt::SerialKillerKill), but(&[1]));
    assert_eq!(prompt_locale_key(Prompt::SerialKillerKill), Some("AskKill"));
    assert_eq!(menu(2, Prompt::SeerCheck), but(&[2]));
    assert_eq!(menu(3, Prompt::WolfEat), but(&[3, 4, 5]));
    assert_eq!(menu(4, Prompt::WolfEat), but(&[3, 4, 5]));
    assert_eq!(menu(6, Prompt::CultistConvert), but(&[6, 7]));
    assert_eq!(menu(8, Prompt::CupidLink), all);
    assert_eq!(menu(9, Prompt::ArsonistDouse), but(&[9]));
    assert_eq!(prompt_locale_key(Prompt::BlacksmithSilver), Some("SpreadDust"));
}

#[tokio::test]
async fn the_serial_killer_kills_at_night() {
    let players = roster(&[
        Role::SerialKiller,
        Role::Villager,
        Role::Villager,
        Role::Villager,
        Role::Wolf,
    ]);
    let mut night = Night::new(&[(Prompt::SerialKillerKill, P2)]);
    play(&players, &mut night, 0, 1).await;
    assert_eq!(night.asked_on(P1, Prompt::SerialKillerKill), vec![1]);
    assert!(
        night.narrated.iter().any(
            |e| matches!(e, NarrationEvent::Death { victim, method: KillMethod::SerialKilled, .. } if *victim == P2)
        ),
        "{:#?}",
        night.narrated
    );
}

#[tokio::test]
async fn teammates_hear_each_vote_as_it_is_cast() {
    use game_engine::orchestrator::{Phase, Question};
    let players = roster(&[
        Role::Villager, // P1
        Role::Villager, // P2
        Role::Wolf,     // P3
        Role::Wolf,     // P4
        Role::SnowWolf, // P5
        Role::Cultist,  // P6
        Role::Cultist,  // P7
        Role::Villager, // P8
    ]);
    let mut presenter = recorder(|phase, _, q| match q {
        Question::Targets { player, .. } if phase == Phase::Night && (player.0 == 3 || player.0 == 6) => Some(P2),
        _ => None,
    });
    let rules = GameRules::default();
    run_game_with_rules(
        &players,
        ALL_ROLES,
        &rules,
        &mut presenter,
        &mut SeededRngSource::new(0),
        1,
    )
    .await;
    let told = |to: u64| -> Vec<Message> {
        presenter
            .narrated
            .iter()
            .filter_map(|e| match e {
                NarrationEvent::Tell { to: t, message } if t.0 == to && message.key.contains("Voted") => {
                    Some(message.clone())
                }
                _ => None,
            })
            .collect()
    };
    let vote = |key, voter| Message::new(key, vec![MessageArg::Player(voter), MessageArg::Player(P2)]);
    assert_eq!(told(4), vec![vote("WolfVotedKill", P3)]);
    assert_eq!(told(7), vec![vote("CultistVotedConvert", PlayerId(6))]);
    assert!(told(5).is_empty(), "the Snow Wolf doesn't vote, and isn't told");
    assert!(
        told(3).is_empty() && told(6).is_empty(),
        "voters aren't told their own vote"
    );
}

#[tokio::test]
async fn the_second_kill_is_asked_of_every_wolf_at_once() {
    use game_engine::orchestrator::{Phase, Question};
    let players = roster(&[
        Role::WolfCub,  // P1
        Role::Wolf,     // P2
        Role::Wolf,     // P3
        Role::Villager, // P4
        Role::Villager, // P5
        Role::Villager, // P6
        Role::Villager, // P7
        Role::Villager, // P8
    ]);
    // Lynch the cub on day 1; on night 2 P2 picks P4 for both kills.
    let mut presenter = recorder(|phase, round, q| match q {
        Question::Targets {
            prompt: Prompt::LynchVote,
            options,
            ..
        } if round == 1 && options.contains(&P1) => Some(P1),
        Question::Targets {
            player,
            prompt: Prompt::WolfCubBonusKill,
            ..
        } if phase == Phase::Night && *player == P2 => Some(PlayerId(5)),
        _ => None,
    });
    let rules = GameRules::default();
    run_game_with_rules(
        &players,
        ALL_ROLES,
        &rules,
        &mut presenter,
        &mut SeededRngSource::new(0),
        2,
    )
    .await;
    let second: Vec<_> = presenter
        .batches
        .iter()
        .filter(|(_, _, qs)| qs.iter().any(|q| q.prompt() == Prompt::WolfCubBonusKill))
        .collect();
    assert_eq!(second.len(), 1, "one round of second-kill questions");
    let mut asked: Vec<PlayerId> = second[0].2.iter().map(|q| q.player()).collect();
    asked.sort_by_key(|p| p.0);
    assert_eq!(asked, vec![P2, P3]);
    let heard = Message::new(
        "WolfVotedKill",
        vec![MessageArg::Player(P2), MessageArg::Player(PlayerId(5))],
    );
    assert!(
        presenter
            .narrated
            .contains(&NarrationEvent::Tell { to: P3, message: heard }),
        "{:#?}",
        presenter.narrated
    );
}
