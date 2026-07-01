//! End-to-end proof that the dependency-ordered orchestrator actually
//! works, not just that individual role files do. Uses a small scripted
//! `Presenter` — standing in for the same role `sim`'s test-double
//! presenter or a real Telegram presenter would play — to drive a full
//! night: two Wolves vote, the orchestrator tallies a majority target,
//! and only then asks Witch her heal question with that target filled in.

use async_trait::async_trait;
use game_engine::orchestrator::{resolve_night, NightPlayer, Presenter, Prompt};
use game_engine::roles::{NightAction, PlayerId, RoleState};
use shared::Role;
use std::collections::HashMap;

/// A presenter with every answer decided in advance, keyed by (player,
/// prompt). Standing in for wherever real answers would come from —
/// Telegram, or a replayed historical game — without the orchestrator
/// under test ever knowing the difference.
struct ScriptedPresenter {
    one_target_answers: HashMap<(PlayerId, Prompt), PlayerId>,
}

#[async_trait]
impl Presenter for ScriptedPresenter {
    async fn ask_targets(
        &mut self,
        player: PlayerId,
        prompt: Prompt,
        _options: &[PlayerId],
        count: usize,
    ) -> Option<Vec<PlayerId>> {
        // None of these tests exercise Cupid's two-target ask; only the
        // one-target case is scripted.
        if count != 1 {
            return None;
        }
        self.one_target_answers
            .get(&(player, prompt))
            .copied()
            .map(|p| vec![p])
    }
}

const WOLF_A: PlayerId = PlayerId(1);
const WOLF_B: PlayerId = PlayerId(2);
const VICTIM: PlayerId = PlayerId(3);
const WITCH: PlayerId = PlayerId(4);
const SEER: PlayerId = PlayerId(5);

#[tokio::test]
async fn witch_is_asked_only_after_the_wolves_target_resolves() {
    let players = vec![
        NightPlayer { id: WOLF_A, role: Role::Wolf },
        NightPlayer { id: WOLF_B, role: Role::Wolf },
        NightPlayer { id: VICTIM, role: Role::Villager },
        NightPlayer { id: WITCH, role: Role::Witch },
        NightPlayer { id: SEER, role: Role::Seer },
    ];

    let mut answers = HashMap::new();
    // Both wolves agree on the same target -> unambiguous majority.
    answers.insert((WOLF_A, Prompt::WolfEat), VICTIM);
    answers.insert((WOLF_B, Prompt::WolfEat), VICTIM);
    // Witch's heal choice matches the (not-yet-known-to-her) wolf target.
    answers.insert((WITCH, Prompt::WitchHeal), VICTIM);
    // Seer checks the victim, just to prove stage 1 runs independent
    // roles too, not just the wolves.
    answers.insert((SEER, Prompt::SeerCheck), VICTIM);

    let mut presenter = ScriptedPresenter {
        one_target_answers: answers,
    };
    let mut states: HashMap<PlayerId, RoleState> = HashMap::new();

    let (actions, wolf_target) = resolve_night(&players, &mut states, &mut presenter).await;

    assert_eq!(wolf_target, Some(VICTIM), "majority wolf vote should resolve");

    assert!(
        actions.contains(&NightAction::EatVote { target: VICTIM }),
        "expected wolf eat votes in the action list: {actions:?}"
    );
    assert!(
        actions.contains(&NightAction::CheckTeam { target: VICTIM }),
        "expected the Seer's independent check to resolve in stage 1: {actions:?}"
    );
    assert!(
        actions.contains(&NightAction::Heal { target: VICTIM }),
        "Witch's heal should fire once the wolf target she depends on is resolved: {actions:?}"
    );
}

#[tokio::test]
async fn witch_heal_does_not_fire_if_her_choice_disagrees_with_the_resolved_wolf_target() {
    let players = vec![
        NightPlayer { id: WOLF_A, role: Role::Wolf },
        NightPlayer { id: VICTIM, role: Role::Villager },
        NightPlayer { id: WITCH, role: Role::Witch },
    ];

    let mut answers = HashMap::new();
    answers.insert((WOLF_A, Prompt::WolfEat), VICTIM);
    // Witch guesses a different (nonexistent) target than the real wolf pick.
    answers.insert((WITCH, Prompt::WitchHeal), PlayerId(99));

    let mut presenter = ScriptedPresenter {
        one_target_answers: answers,
    };
    let mut states: HashMap<PlayerId, RoleState> = HashMap::new();

    let (actions, wolf_target) = resolve_night(&players, &mut states, &mut presenter).await;

    assert_eq!(wolf_target, Some(VICTIM));
    assert!(
        !actions.iter().any(|a| matches!(a, NightAction::Heal { .. })),
        "heal shouldn't fire when it targets someone other than the resolved wolf target: {actions:?}"
    );
}

#[tokio::test]
async fn a_tied_wolf_vote_resolves_to_no_target() {
    let wolf_c = PlayerId(6);
    let other_victim = PlayerId(7);
    let players = vec![
        NightPlayer { id: WOLF_A, role: Role::Wolf },
        NightPlayer { id: wolf_c, role: Role::Wolf },
        NightPlayer { id: VICTIM, role: Role::Villager },
        NightPlayer { id: other_victim, role: Role::Villager },
    ];

    let mut answers = HashMap::new();
    answers.insert((WOLF_A, Prompt::WolfEat), VICTIM);
    answers.insert((wolf_c, Prompt::WolfEat), other_victim);

    let mut presenter = ScriptedPresenter {
        one_target_answers: answers,
    };
    let mut states: HashMap<PlayerId, RoleState> = HashMap::new();

    let (_actions, wolf_target) = resolve_night(&players, &mut states, &mut presenter).await;
    assert_eq!(wolf_target, None, "a tied vote should not resolve to either target");
}
