//! Ties the individual role files together into an actual night
//! resolution, without ever knowing where questions go or answers come
//! from. That's the `Presenter` trait: the orchestrator calls
//! `ask_targets` and gets answers back, and has no idea whether it's
//! talking to a real Telegram chat or a scripted test harness. `sim` and
//! (eventually) `control` each implement `Presenter` their own way; this
//! file never depends on either.
//!
//! `Presenter` has exactly one question-asking method, parameterized by
//! how many targets are wanted, rather than one method per arity
//! (`ask_target`, `ask_two_targets`, ...). A one-per-arity design is fine
//! for exactly one extra case, but it doesn't stop there — the moment a
//! third arity shows up (a role linking three players, say), every
//! `Presenter` implementation would need a matching new method forever.
//! Collapsing to `ask_targets(.., count)` means "how many" is just a
//! number, and implementers (a real Telegram adapter, a test double)
//! never need to know or care how many roles-that-pick-N-players exist.
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
    /// Ask `player` to pick `count` distinct players from `options` for
    /// the given `prompt`. Returns `None` if the player declines/times out
    /// (a legitimate answer, e.g. Harlot choosing to stay home) — an
    /// implementer that returns `Some(v)` should return exactly `count`
    /// entries; the orchestrator treats anything else as "no answer"
    /// rather than trusting a malformed response.
    async fn ask_targets(
        &mut self,
        player: PlayerId,
        prompt: Prompt,
        options: &[PlayerId],
        count: usize,
    ) -> Option<Vec<PlayerId>>;
}

/// Thin convenience wrapper for the overwhelmingly common case (one
/// target), so call sites in `resolve_night` read as "ask for one player"
/// rather than "ask for a list and hope it has one element." Not a trait
/// method — implementers only ever deal with `ask_targets`.
async fn ask_one(
    presenter: &mut dyn Presenter,
    player: PlayerId,
    prompt: Prompt,
    options: &[PlayerId],
) -> Option<PlayerId> {
    let mut picked = presenter.ask_targets(player, prompt, options, 1).await?;
    if picked.len() == 1 {
        picked.pop()
    } else {
        None
    }
}

/// Same idea for exactly two targets (Cupid today).
async fn ask_two(
    presenter: &mut dyn Presenter,
    player: PlayerId,
    prompt: Prompt,
    options: &[PlayerId],
) -> Option<(PlayerId, PlayerId)> {
    let picked = presenter.ask_targets(player, prompt, options, 2).await?;
    match picked.as_slice() {
        [a, b] if a != b => Some((*a, *b)),
        _ => None,
    }
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
            let chosen = ask_two(presenter, player.id, Prompt::CupidLink, &alive).await;
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
            Some(prompt) => ask_one(presenter, player.id, prompt, &alive).await,
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
        let heal_target = ask_one(presenter, player.id, Prompt::WitchHeal, &alive).await;
        let poison_target = ask_one(presenter, player.id, Prompt::WitchPoison, &alive).await;

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
