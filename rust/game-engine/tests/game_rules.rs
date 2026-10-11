//! Whole-game checks for features/game-rules.feature: the engine following
//! the group's timers, secret lynch, show-roles-on-death and ShowRolesEnd.

use async_trait::async_trait;
use game_engine::messages::{Announcement, SecretTally};
use game_engine::orchestrator::{AlivePlayer, NarrationEvent, Presenter, Prompt};
use game_engine::rng::SeededRngSource;
use game_engine::roles::PlayerId;
use game_engine::rules::{EndRoles, GameRules, SecretLynch};
use game_engine::run_game_with_rules;
use shared::{Role, ALL_ROLES};

const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);
const P3: PlayerId = PlayerId(3);

/// Everyone but the Seer (P2) votes for the Seer, who votes for the Wolf
/// (P1); the Mayor (P3) reveals. Nobody acts at night.
#[derive(Default)]
struct Village {
    narrated: Vec<NarrationEvent>,
}

impl Village {
    fn announcements(&self) -> Vec<Announcement> {
        self.narrated
            .iter()
            .filter_map(|e| match e {
                NarrationEvent::Announce(a) => Some(a.clone()),
                _ => None,
            })
            .collect()
    }
}

#[async_trait(?Send)]
impl Presenter for Village {
    async fn ask_targets(
        &mut self,
        player: PlayerId,
        prompt: Prompt,
        _: &[PlayerId],
        _: usize,
    ) -> Option<Vec<PlayerId>> {
        match prompt {
            Prompt::LynchVote => Some(vec![if player == P2 { P1 } else { P2 }]),
            _ => None,
        }
    }

    async fn ask_toggle(&mut self, player: PlayerId, prompt: Prompt) -> bool {
        player == P3 && prompt == Prompt::MayorReveal
    }

    async fn narrate(&mut self, event: NarrationEvent) {
        self.narrated.push(event);
    }
}

fn village(extra: Role) -> Vec<AlivePlayer> {
    let mut roles = vec![
        Role::Wolf,
        Role::Seer,
        Role::Mayor,
        Role::Villager,
        Role::Villager,
        Role::Villager,
        Role::Villager,
    ];
    roles.push(extra);
    roles
        .iter()
        .enumerate()
        .map(|(i, &role)| AlivePlayer {
            id: PlayerId(i as u64 + 1),
            role,
        })
        .collect()
}

async fn play(rules: GameRules, extra: Role) -> Village {
    let mut presenter = Village::default();
    run_game_with_rules(
        &village(extra),
        ALL_ROLES,
        &rules,
        &mut presenter,
        &mut SeededRngSource::new(0),
        1,
    )
    .await;
    presenter
}

#[tokio::test]
async fn phase_timers_are_announced_with_the_groups_durations() {
    let mut rules = GameRules::default();
    rules.timers.night_seconds = 60;
    let plain = play(rules, Role::Villager).await.announcements();
    assert_eq!(plain[0], Announcement::NightTime { seconds: 60 });
    let day: Vec<_> = plain
        .iter()
        .skip_while(|a| !matches!(a, Announcement::DayTime { .. }))
        .take(3)
        .cloned()
        .collect();
    assert_eq!(
        day,
        vec![
            Announcement::DayTime { seconds: 120 },
            Announcement::Day { number: 1 },
            Announcement::LynchTime { seconds: 90 }
        ]
    );

    let with_cupid = play(rules, Role::Cupid).await.announcements();
    assert_eq!(with_cupid[0], Announcement::NightTime { seconds: 120 });
}

#[tokio::test]
async fn public_votes_are_announced_as_they_are_cast() {
    let announced = play(GameRules::default(), Role::Villager).await.announcements();
    assert!(announced.contains(&Announcement::PlayerVotedLynch { voter: P1, target: P2 }));
    assert!(announced.contains(&Announcement::PlayerVotedLynch { voter: P2, target: P1 }));
    assert!(!announced.iter().any(|a| matches!(
        a,
        Announcement::PlayerVoteCounts { .. } | Announcement::SecretLynchResults(_)
    )));
}

fn secret(show_votes: bool, show_voters: bool) -> GameRules {
    GameRules {
        secret_lynch: Some(SecretLynch {
            show_votes,
            show_voters,
        }),
        ..GameRules::default()
    }
}

#[tokio::test]
async fn a_secret_lynch_only_shows_how_many_have_voted() {
    let announced = play(secret(false, false), Role::Villager).await.announcements();
    let counts: Vec<_> = announced
        .iter()
        .filter_map(|a| match a {
            Announcement::PlayerVoteCounts { voted, alive } => Some((*voted, *alive)),
            _ => None,
        })
        .collect();
    assert_eq!(counts, (1..=8).map(|n| (n, 8)).collect::<Vec<_>>());
    assert!(!announced.iter().any(|a| matches!(
        a,
        Announcement::PlayerVotedLynch { .. } | Announcement::SecretLynchResults(_)
    )));
}

fn secret_results(announced: &[Announcement]) -> Vec<SecretTally> {
    announced
        .iter()
        .find_map(|a| match a {
            Announcement::SecretLynchResults(t) => Some(t.clone()),
            _ => None,
        })
        .expect("the secret lynch results are shown")
}

#[tokio::test]
async fn secret_lynch_results_show_counts_and_optionally_voters() {
    let counts = secret_results(&play(secret(true, false), Role::Villager).await.announcements());
    assert_eq!(
        counts,
        vec![
            SecretTally {
                target: P2,
                votes: 8,
                voters: None
            },
            SecretTally {
                target: P1,
                votes: 1,
                voters: None
            }
        ]
    );

    let voters = secret_results(&play(secret(true, true), Role::Villager).await.announcements());
    assert_eq!(voters[0].votes, 8, "the revealed Mayor counts twice");
    assert!(voters[0].voters.as_ref().unwrap().contains(&(P3, 2)));
    assert_eq!(voters[1].voters, Some(vec![(P2, 1)]));
}

fn seer_death_reveals(village: &Village) -> bool {
    village
        .narrated
        .iter()
        .find_map(|e| match e {
            NarrationEvent::Death {
                victim, reveal_role, ..
            } if *victim == P2 => Some(*reveal_role),
            _ => None,
        })
        .expect("the Seer is lynched")
}

#[tokio::test]
async fn roles_are_revealed_on_death_only_when_the_group_allows_it() {
    assert!(seer_death_reveals(&play(GameRules::default(), Role::Villager).await));
    let hidden = GameRules {
        show_roles_on_death: false,
        ..GameRules::default()
    };
    assert!(!seer_death_reveals(&play(hidden, Role::Villager).await));
}

#[tokio::test]
async fn the_game_ends_with_a_summary_following_show_roles_end() {
    for end_roles in [EndRoles::None, EndRoles::Living, EndRoles::All] {
        let village = play(
            GameRules {
                end_roles,
                ..GameRules::default()
            },
            Role::Villager,
        )
        .await;
        let summary = village
            .narrated
            .iter()
            .find_map(|e| match e {
                NarrationEvent::GameSummary(s) => Some(s.clone()),
                _ => None,
            })
            .expect("a summary closes the game");
        assert_eq!(summary.end_roles, end_roles);
        assert_eq!(summary.entries.len(), 8);
        let seer = summary.entries.iter().find(|e| e.id == P2).unwrap();
        assert!(!seer.alive);
        assert_eq!(seer.role, Role::Seer);
        assert_eq!(summary.entries.iter().filter(|e| e.alive).count(), 7);
    }
}
