//! Proves the Traitor and Wild Child transforms actually happen mid-game
//! and change the outcome — not just that `apply_transforms` mutates a
//! role in isolation (that's covered by unit tests in `game.rs`), but that
//! `run_game` produces a *different winner* than it would without them.

use async_trait::async_trait;
use game_engine::orchestrator::{AlivePlayer, Presenter, Prompt};
use game_engine::roles::PlayerId;
use game_engine::{run_game, WinOutcome};
use shared::{Role, Team};
use std::collections::HashMap;

struct FixedScriptPresenter {
    wolf_eats: HashMap<PlayerId, PlayerId>,
    lynches: PlayerId,
    wild_child_picks: PlayerId,
}

#[async_trait]
impl Presenter for FixedScriptPresenter {
    async fn ask_targets(
        &mut self,
        player: PlayerId,
        prompt: Prompt,
        _options: &[PlayerId],
        count: usize,
    ) -> Option<Vec<PlayerId>> {
        if count != 1 {
            return None;
        }
        match prompt {
            Prompt::WolfEat => self.wolf_eats.get(&player).copied().map(|t| vec![t]),
            Prompt::LynchVote => Some(vec![self.lynches]),
            Prompt::WildChildRoleModel => Some(vec![self.wild_child_picks]),
            _ => None,
        }
    }
}

/// The last Wolf dying should turn the Traitor into a Wolf, letting the
/// (now two-strong) wolf side outnumber the remaining village — a win
/// this composition couldn't reach if the Traitor stayed inert.
#[tokio::test]
async fn traitor_becomes_a_wolf_once_the_last_wolf_dies_and_helps_wolves_win() {
    let wolf = PlayerId(1);
    let traitor = PlayerId(2);
    let villager1 = PlayerId(3);
    let villager2 = PlayerId(4);

    let players = vec![
        AlivePlayer {
            id: wolf,
            role: Role::Wolf,
        },
        AlivePlayer {
            id: traitor,
            role: Role::Traitor,
        },
        AlivePlayer {
            id: villager1,
            role: Role::Villager,
        },
        AlivePlayer {
            id: villager2,
            role: Role::Villager,
        },
    ];

    // Night 1: the wolf eats nobody in particular (no wolf_eats entry, so
    // no target) - instead the village lynches the wolf on day 1. That
    // leaves 3 alive: Traitor, villager1, villager2, no wolf muscle -> the
    // Traitor should transform. Night 2: the (now-Wolf) Traitor eats
    // villager1, leaving Traitor(Wolf) vs villager2 - 1 vs 1, wolves win.
    let mut presenter = FixedScriptPresenter {
        wolf_eats: HashMap::from([(traitor, villager1)]), // only matters once traitor IS a wolf
        lynches: wolf,
        wild_child_picks: PlayerId(0), // unused here
    };

    let outcome = run_game(&players, &mut presenter, 10).await;

    assert_eq!(outcome.winner, WinOutcome::Team(Team::Wolf));
    // Sanity: the Wolf died to the day-1 lynch, and villager1 to the
    // Traitor-turned-Wolf's night-2 eat - both deaths should be present.
    assert!(outcome
        .deaths
        .iter()
        .any(|&(id, _)| id == wolf));
    assert!(outcome
        .deaths
        .iter()
        .any(|&(id, _)| id == villager1));
}

/// Wild Child's role model dying should turn her into a Wolf.
#[tokio::test]
async fn wild_child_becomes_a_wolf_once_her_role_model_dies() {
    let wolf = PlayerId(1);
    let wild_child = PlayerId(2);
    let role_model = PlayerId(3);
    let bystander = PlayerId(4);

    let players = vec![
        AlivePlayer {
            id: wolf,
            role: Role::Wolf,
        },
        AlivePlayer {
            id: wild_child,
            role: Role::WildChild,
        },
        AlivePlayer {
            id: role_model,
            role: Role::Villager,
        },
        AlivePlayer {
            id: bystander,
            role: Role::Villager,
        },
    ];

    // Wild Child picks role_model as her role model night 1; the wolf
    // eats role_model that same night, turning Wild Child into a Wolf
    // immediately after. From then on: wolf + (Wild Child as Wolf) vs
    // bystander - 2 wolves outnumber 1 villager.
    let mut presenter = FixedScriptPresenter {
        wolf_eats: HashMap::from([(wolf, role_model)]),
        lynches: PlayerId(999), // nobody real - no-op lynch vote
        wild_child_picks: role_model,
    };

    let outcome = run_game(&players, &mut presenter, 10).await;

    assert_eq!(outcome.winner, WinOutcome::Team(Team::Wolf));
    assert_eq!(outcome.rounds_played, 1);
}
