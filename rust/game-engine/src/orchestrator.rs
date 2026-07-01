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

/// The one place that validates a presenter's answer actually matches what
/// was asked for: exactly `count` entries, all distinct. Every call site
/// below goes through this — including `ask_one` — so "what counts as a
/// well-formed answer" is defined once, not re-derived per arity.
async fn ask_exact(
    presenter: &mut dyn Presenter,
    player: PlayerId,
    prompt: Prompt,
    options: &[PlayerId],
    count: usize,
) -> Option<Vec<PlayerId>> {
    let picked = presenter.ask_targets(player, prompt, options, count).await?;
    let all_distinct = (1..picked.len()).all(|i| !picked[..i].contains(&picked[i]));
    if picked.len() == count && all_distinct {
        Some(picked)
    } else {
        None
    }
}

/// Convenience for the one-target case, since it's by far the common one
/// (every role in `one_target_prompt` below, plus both of Witch's potion
/// questions — nine call sites and counting). Cupid's two-target ask has
/// exactly one call site, so it isn't given the same treatment: naming a
/// function for something called once is the same mistake as `ask_target`/
/// `ask_two_targets` being separate trait methods, just moved down a
/// layer — it goes straight through `ask_exact` at its call site instead.
async fn ask_one(
    presenter: &mut dyn Presenter,
    player: PlayerId,
    prompt: Prompt,
    options: &[PlayerId],
) -> Option<PlayerId> {
    ask_exact(presenter, player, prompt, options, 1)
        .await
        .and_then(|v| v.into_iter().next())
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
            let chosen = ask_exact(presenter, player.id, Prompt::CupidLink, &alive, 2)
                .await
                .map(|v| (v[0], v[1]));
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

/// Turns resolved night decisions into actual deaths — the step
/// `resolve_night`'s own doc comment flagged as future work. Deliberately
/// narrow: only the two cases this codebase's role logic actually models
/// resolve to a death.
///
/// - The wolves' `wolf_target` dies of `KillMethod::Eat`, **unless**
///   Witch's `Heal` action names that exact same player (that's the whole
///   point of the heal potion — see `witch` module).
/// - Witch's `Poison` target dies of `KillMethod::Poison`, unconditionally
///   and independently of the wolf kill.
///
/// Everything else this proof-of-concept resolves as a *decision*
/// (`Visit`, `Protect`, `Investigate`, `CheckTeam`, `ChooseRoleModel`) has
/// no death consequence modeled yet — Harlot dying from visiting a wolf,
/// Guardian Angel's protection actually working or the GA dying instead,
/// are real legacy mechanics (see `harlot`/`guardian_angel` module docs)
/// that need cross-player resolution logic this function doesn't attempt.
/// If the wolf target and the poison target are the same player, they
/// appear twice, once per cause — deduplicating "someone already dead"
/// is future work for whoever applies this to real running game state.
pub fn apply_night_results(
    actions: &[NightAction],
    wolf_target: Option<PlayerId>,
) -> Vec<(PlayerId, shared::KillMethod)> {
    let mut deaths = vec![];

    let healed_target = actions.iter().find_map(|a| match a {
        NightAction::Heal { target } => Some(*target),
        _ => None,
    });

    if let Some(target) = wolf_target {
        if healed_target != Some(target) {
            deaths.push((target, shared::KillMethod::Eat));
        }
    }

    for action in actions {
        if let NightAction::Poison { target } = action {
            deaths.push((*target, shared::KillMethod::Poison));
        }
    }

    deaths
}

#[cfg(test)]
mod apply_night_results_tests {
    use super::*;
    use shared::KillMethod;

    #[test]
    fn wolf_target_dies_when_nobody_heals() {
        let target = PlayerId(1);
        let deaths = apply_night_results(&[], Some(target));
        assert_eq!(deaths, vec![(target, KillMethod::Eat)]);
    }

    #[test]
    fn heal_on_the_wolf_target_cancels_the_kill() {
        let target = PlayerId(1);
        let actions = [NightAction::Heal { target }];
        let deaths = apply_night_results(&actions, Some(target));
        assert_eq!(deaths, vec![], "healing the wolf's own target should cancel it");
    }

    #[test]
    fn heal_on_someone_else_does_not_cancel_the_wolf_kill() {
        let target = PlayerId(1);
        let actions = [NightAction::Heal {
            target: PlayerId(2),
        }];
        let deaths = apply_night_results(&actions, Some(target));
        assert_eq!(deaths, vec![(target, KillMethod::Eat)]);
    }

    #[test]
    fn poison_kills_independently_of_the_wolf_target() {
        let wolf_target = PlayerId(1);
        let poisoned = PlayerId(2);
        let actions = [NightAction::Poison { target: poisoned }];
        let mut deaths = apply_night_results(&actions, Some(wolf_target));
        deaths.sort_by_key(|(id, _)| id.0);
        assert_eq!(
            deaths,
            vec![
                (wolf_target, KillMethod::Eat),
                (poisoned, KillMethod::Poison)
            ]
        );
    }

    #[test]
    fn no_wolf_target_means_no_eat_death() {
        assert_eq!(apply_night_results(&[], None), vec![]);
    }
}
