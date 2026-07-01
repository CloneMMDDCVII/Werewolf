//! Ties the individual role files together into an actual night
//! resolution, without ever knowing where questions go or answers come
//! from. That's the `Presenter` trait: the orchestrator calls
//! `ask_target`/`ask_two_targets` and gets an answer back, and has no idea
//! whether it's talking to a real Telegram chat or a scripted test
//! harness. `sim` and (eventually) `control` each implement `Presenter`
//! their own way; this file never depends on either.
//!
//! Resolution happens in dependency order, derived from
//! `RoleBehavior::requires()` rather than a hardcoded phase list: every
//! role whose dependencies are empty is asked in one batch first, then
//! anything depending on a fact produced by that batch (currently just
//! `NightFact::WolfTarget`, produced by tallying `NightAction::EatVote`)
//! is asked in a second batch with that fact filled in. This is
//! deliberately only two levels deep for now — there's exactly one
//! dependent role (`Witch`) and one fact — generalizing to a real N-level
//! topological sort is future work for whenever a second dependency shows
//! up and two hardcoded levels stop being enough.

use crate::roles::{
    behavior_for, NightAction, NightContext, NightFact, PlayerId, RoleState,
};
use async_trait::async_trait;
use shared::Role;
use std::collections::HashMap;

/// Identifies *why* a player is being asked something, so a presenter can
/// look up the right prompt text (via `i18n`) without the orchestrator
/// needing to know anything about message formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Prompt {
    WolfEat,
    SeerCheck,
    HarlotVisit,
    GuardianAngelProtect,
    DetectiveInvestigate,
    FoolInvestigate,
    WildChildRoleModel,
    CupidLink,
    WitchHeal,
    WitchPoison,
}

/// The seam: real I/O (or a test double) lives entirely behind this trait.
/// Async because a real Telegram presenter is fundamentally waiting on
/// network events (a callback query arriving), not something that can be
/// answered synchronously.
#[async_trait]
pub trait Presenter {
    /// Ask `player` to pick one of `options` for the given `prompt`.
    /// Returns `None` if the player declines/times out — declining is a
    /// legitimate answer (e.g. Harlot choosing to stay home), not an error.
    async fn ask_target(
        &mut self,
        player: PlayerId,
        prompt: Prompt,
        options: &[PlayerId],
    ) -> Option<PlayerId>;

    /// Cupid-shaped question: pick two distinct players from `options`.
    async fn ask_two_targets(
        &mut self,
        player: PlayerId,
        prompt: Prompt,
        options: &[PlayerId],
    ) -> Option<(PlayerId, PlayerId)>;
}

/// What every alive player brings into a night: who they are and what
/// role they're currently playing.
#[derive(Debug, Clone, Copy)]
pub struct NightPlayer {
    pub id: PlayerId,
    pub role: Role,
}

/// Maps a role to the shape of question it needs asked, if any. Roles with
/// no entry here (Villager, Drunk, Traitor, Cursed, Tanner — see their
/// module docs) have no night question at all; `night_action` is still
/// called for them with an empty context, which is harmless since their
/// implementations don't look at it.
fn one_target_prompt(role: Role) -> Option<Prompt> {
    match role {
        Role::Wolf => Some(Prompt::WolfEat),
        Role::Seer => Some(Prompt::SeerCheck),
        Role::Harlot => Some(Prompt::HarlotVisit),
        Role::GuardianAngel => Some(Prompt::GuardianAngelProtect),
        Role::Detective => Some(Prompt::DetectiveInvestigate),
        Role::Fool => Some(Prompt::FoolInvestigate),
        Role::WildChild => Some(Prompt::WildChildRoleModel),
        _ => None,
    }
}

/// Resolves one full night: asks every independent role first, tallies
/// the wolves' target, then asks dependent roles (currently just Witch)
/// with that fact available. Returns every `NightAction` produced, plus
/// the resolved wolf target for the orchestrator's caller to apply deaths
/// with (that application step — actually killing someone — is future
/// work; this function only resolves *decisions*).
pub async fn resolve_night(
    players: &[NightPlayer],
    states: &mut HashMap<PlayerId, RoleState>,
    presenter: &mut dyn Presenter,
) -> (Vec<NightAction>, Option<PlayerId>) {
    let alive: Vec<PlayerId> = players.iter().map(|p| p.id).collect();
    let mut actions = vec![];

    // --- Stage 1: roles with no dependencies ---
    for player in players {
        let behavior = behavior_for(player.role);
        if !behavior.requires().is_empty() {
            continue; // handled in stage 2
        }

        let state = states.entry(player.id).or_default();

        if player.role == Role::Cupid {
            let chosen = presenter
                .ask_two_targets(player.id, Prompt::CupidLink, &alive)
                .await;
            let ctx = NightContext {
                alive: &alive,
                self_id: player.id,
                chosen_target: None,
                heal_target: None,
                poison_target: None,
                love_targets: chosen,
                wolf_target: None,
            };
            actions.extend(behavior.night_action(&ctx, state));
            continue;
        }

        let chosen_target = match one_target_prompt(player.role) {
            Some(prompt) => presenter.ask_target(player.id, prompt, &alive).await,
            None => None,
        };

        let ctx = NightContext {
            alive: &alive,
            self_id: player.id,
            chosen_target,
            heal_target: None,
            poison_target: None,
            love_targets: None,
            wolf_target: None,
        };
        actions.extend(behavior.night_action(&ctx, state));
    }

    // Tally the wolves' target: majority vote among EatVote actions, tie
    // or no votes resolves to None. Real tie-breaking rules (if any) are
    // future work — this is enough to unblock stage 2's dependency.
    let wolf_target = majority_eat_target(&actions);

    // --- Stage 2: roles depending on a fact produced above ---
    for player in players {
        let behavior = behavior_for(player.role);
        if behavior.requires().is_empty() {
            continue; // already handled in stage 1
        }
        debug_assert!(
            behavior.requires().contains(&NightFact::WolfTarget),
            "stage 2 currently only understands WolfTarget-dependent roles"
        );

        let state = states.entry(player.id).or_default();

        // Witch is the only stage-2 role today: ask both her potion
        // questions. `is_available()` is a role-assignment-time concern
        // (whether she's dealt into the game at all), not enforced here —
        // if she's in `players`, the orchestrator resolves her correctly
        // regardless, which is what proves the dependency plumbing works.
        let heal_target = presenter
            .ask_target(player.id, Prompt::WitchHeal, &alive)
            .await;
        let poison_target = presenter
            .ask_target(player.id, Prompt::WitchPoison, &alive)
            .await;

        let ctx = NightContext {
            alive: &alive,
            self_id: player.id,
            chosen_target: None,
            heal_target,
            poison_target,
            love_targets: None,
            wolf_target,
        };
        actions.extend(behavior.night_action(&ctx, state));
    }

    (actions, wolf_target)
}

fn majority_eat_target(actions: &[NightAction]) -> Option<PlayerId> {
    let mut counts: HashMap<PlayerId, usize> = HashMap::new();
    for action in actions {
        if let NightAction::EatVote { target } = action {
            *counts.entry(*target).or_insert(0) += 1;
        }
    }
    let max_count = *counts.values().max()?;
    let mut leaders = counts.iter().filter(|&(_, &c)| c == max_count);
    let first = leaders.next()?;
    if leaders.next().is_some() {
        None // tie
    } else {
        Some(*first.0)
    }
}
