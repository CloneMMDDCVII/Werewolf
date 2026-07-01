//! Day-phase counterpart to `night_orchestration.rs`, plus the payoff of
//! having both a real day resolver and a real win-condition evaluator
//! already built: lynching a Tanner should trip the win-condition's
//! short-circuit (`evaluate_winner_with_kills`, ported from
//! Werewolf.cs:2768-2776) all the way from a scripted day vote, through
//! `resolve_day` and `apply_day_results`, without any of those three
//! pieces having been written with each other in mind.

use async_trait::async_trait;
use game_engine::orchestrator::{apply_day_results, resolve_day, AlivePlayer, Presenter, Prompt};
use game_engine::roles::PlayerId;
use game_engine::{evaluate_winner_with_kills, KillEvent, PlayerState, WinOutcome};
use shared::{Role, Team};
use std::collections::HashMap;

struct ScriptedDayPresenter {
    votes: HashMap<PlayerId, PlayerId>,
}

#[async_trait]
impl Presenter for ScriptedDayPresenter {
    async fn ask_targets(
        &mut self,
        player: PlayerId,
        prompt: Prompt,
        _options: &[PlayerId],
        count: usize,
    ) -> Option<Vec<PlayerId>> {
        if count != 1 || prompt != Prompt::LynchVote {
            return None;
        }
        self.votes.get(&player).copied().map(|t| vec![t])
    }
}

#[tokio::test]
async fn lynching_a_tanner_wins_the_game_for_tanner_end_to_end() {
    let voter_a = PlayerId(1);
    let voter_b = PlayerId(2);
    let voter_c = PlayerId(3);
    let tanner = PlayerId(4);

    let players = vec![
        AlivePlayer {
            id: voter_a,
            role: Role::Villager,
        },
        AlivePlayer {
            id: voter_b,
            role: Role::Villager,
        },
        AlivePlayer {
            id: voter_c,
            role: Role::Villager,
        },
        AlivePlayer {
            id: tanner,
            role: Role::Tanner,
        },
    ];

    // Everyone votes to lynch the Tanner, unaware that's exactly what he wants.
    let mut votes = HashMap::new();
    votes.insert(voter_a, tanner);
    votes.insert(voter_b, tanner);
    votes.insert(voter_c, tanner);
    let mut presenter = ScriptedDayPresenter { votes };
    let mut states = HashMap::new();

    let (day_actions, lynch_target) = resolve_day(&players, &mut states, &mut presenter).await;
    assert_eq!(lynch_target, Some(tanner));

    let deaths = apply_day_results(&day_actions, lynch_target);

    // Feed the resolved death into the (previously separately-built)
    // win-condition evaluator, exactly as a real orchestrator loop would.
    let player_states: Vec<PlayerState> = players
        .iter()
        .map(|p| PlayerState {
            id: p.id.0,
            role: p.role,
            alive: !deaths.iter().any(|(victim, _)| *victim == p.id),
        })
        .collect();
    let kill_events: Vec<KillEvent> = deaths
        .iter()
        .map(|(victim, method)| KillEvent {
            victim_role: players.iter().find(|p| p.id == *victim).unwrap().role,
            method: *method,
        })
        .collect();

    let outcome = evaluate_winner_with_kills(&player_states, &kill_events);
    assert_eq!(outcome, WinOutcome::Team(Team::Tanner));
}
