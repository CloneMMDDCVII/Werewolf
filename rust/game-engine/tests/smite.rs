//! `/smite` in a running game (Werewolf.cs::FleePlayer, 5357-5391): the
//! admin's targets reach `run_game` through the same
//! `Presenter::drain_fled_players` seam `/flee` uses, so a smitten player
//! dies exactly as a fleeing one does. These tests pin the cases only an
//! admin can produce: several targets at once, and targets who aren't
//! (or are no longer) alive.
use async_trait::async_trait;
use game_engine::orchestrator::{AlivePlayer, Presenter, Prompt};
use game_engine::rng::SeededRngSource;
use game_engine::roles::PlayerId;
use game_engine::{run_game, WinOutcome};
use shared::{KillMethod, Role, Team};

/// Hands over one batch of smitten ids on the first poll, then nothing.
struct SmitePresenter {
    smitten: Vec<PlayerId>,
}

#[async_trait(?Send)]
impl Presenter for SmitePresenter {
    async fn ask_targets(
        &mut self,
        _player: PlayerId,
        _prompt: Prompt,
        _options: &[PlayerId],
        _count: usize,
    ) -> Option<Vec<PlayerId>> {
        None
    }

    fn drain_fled_players(&mut self) -> Vec<PlayerId> {
        std::mem::take(&mut self.smitten)
    }
}

fn village() -> Vec<AlivePlayer> {
    vec![
        AlivePlayer {
            id: PlayerId(1),
            role: Role::Wolf,
        },
        AlivePlayer {
            id: PlayerId(2),
            role: Role::Villager,
        },
        AlivePlayer {
            id: PlayerId(3),
            role: Role::Villager,
        },
        AlivePlayer {
            id: PlayerId(4),
            role: Role::Villager,
        },
        AlivePlayer {
            id: PlayerId(5),
            role: Role::Villager,
        },
    ]
}

#[tokio::test]
async fn smiting_several_players_kills_them_in_order_and_checks_for_a_win() {
    let mut presenter = SmitePresenter {
        smitten: vec![PlayerId(3), PlayerId(2), PlayerId(4)],
    };
    let outcome = run_game(&village(), &mut presenter, &mut SeededRngSource::new(0), 5).await;

    assert_eq!(
        outcome.deaths,
        vec![
            (PlayerId(3), KillMethod::Flee),
            (PlayerId(2), KillMethod::Flee),
            (PlayerId(4), KillMethod::Flee)
        ]
    );
    assert_eq!(outcome.winner, WinOutcome::Team(Team::Wolf));
    assert_eq!(outcome.rounds_played, 1);
}

#[tokio::test]
async fn smiting_the_last_wolf_hands_the_village_the_win() {
    let mut presenter = SmitePresenter {
        smitten: vec![PlayerId(1)],
    };
    let outcome = run_game(&village(), &mut presenter, &mut SeededRngSource::new(0), 5).await;

    assert_eq!(outcome.deaths, vec![(PlayerId(1), KillMethod::Flee)]);
    assert_eq!(outcome.winner, WinOutcome::Team(Team::Village));
}

#[tokio::test]
async fn smiting_someone_not_in_the_game_changes_nothing() {
    let mut presenter = SmitePresenter {
        smitten: vec![PlayerId(99), PlayerId(2)],
    };
    let outcome = run_game(&village(), &mut presenter, &mut SeededRngSource::new(0), 1).await;

    assert_eq!(outcome.deaths.first(), Some(&(PlayerId(2), KillMethod::Flee)));
    assert!(!outcome.deaths.iter().any(|(id, _)| *id == PlayerId(99)));
}

#[tokio::test]
async fn smiting_the_same_player_twice_kills_them_once() {
    let mut presenter = SmitePresenter {
        smitten: vec![PlayerId(2), PlayerId(2)],
    };
    let outcome = run_game(&village(), &mut presenter, &mut SeededRngSource::new(0), 1).await;

    let flee_deaths = outcome.deaths.iter().filter(|(id, _)| *id == PlayerId(2)).count();
    assert_eq!(flee_deaths, 1, "deaths: {:?}", outcome.deaths);
}
