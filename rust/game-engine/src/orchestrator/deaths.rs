//! Deaths that follow other deaths or need a roll: the Hunter's last
//! shot, a lover's grief, the Spumpkin's detonation and the Grave
//! Digger's trap.

use super::batch::validate_pick;
use super::{AlivePlayer, NarrationEvent, Presenter, Prompt};
use crate::messages::Announcement;
use crate::rng::RngSource;
use crate::roles::{DayAction, LoversState, PlayerId};
use shared::{KillMethod, Role};

/// The Hunter's final shot (Werewolf.cs:5420-5484, `HunterFinalShot`).
/// Called by the game right after a batch of deaths, with the roster
/// already showing who survived it. Each Hunter in `died_with_roles` is
/// asked once, with a different prompt when lynched
/// (`killMethod == KillMthd.Lynch` in legacy), and whoever they pick dies
/// by `KillMethod::HunterShot`.
///
/// A Hunter eaten by the wolves, burned by the Arsonist or fallen into a
/// grave gets no shot: legacy passes `hunterFinalShot: false` there
/// (Werewolf.cs:3419, 3199, 2391). A Hunter who shoots back at the pack
/// does so in `night::Night::wolves_attack` instead.
///
/// Asks each dead Hunter in this batch once. "Domino" chains (a Hunter's
/// shot landing on *another* Hunter, Werewolf.cs:5480-5481) are handled by
/// `game::process_deaths_and_hunter_shots` calling this again on the
/// shots' own deaths.
/// Returns `(target, hunter)` pairs. Announces a
/// Hunter who shoots nobody (Werewolf.cs:5462) and one who shoots the
/// Wise Elder (Werewolf.cs:5473); a shot that kills is announced with the
/// death itself (`death_text`).
pub async fn resolve_hunter_shots(
    new_deaths: &[(PlayerId, KillMethod)],
    died_with_roles: &[(PlayerId, Role)],
    alive: &[AlivePlayer],
    presenter: &mut dyn Presenter,
) -> Vec<(PlayerId, PlayerId)> {
    let alive_ids: Vec<PlayerId> = alive.iter().map(|p| p.id).collect();
    let mut shots = vec![];

    for &(hunter_id, role) in died_with_roles {
        if role != Role::Hunter {
            continue;
        }
        let method = new_deaths
            .iter()
            .find(|&&(id, _)| id == hunter_id)
            .map(|&(_, m)| m)
            .expect("every id in died_with_roles has a matching entry in new_deaths");
        // Werewolf.cs passes `hunterFinalShot: false` for these (3419, 3199, 2391).
        if matches!(method, KillMethod::Eat | KillMethod::Burn | KillMethod::FallGrave) {
            continue;
        }
        let prompt = if method == KillMethod::Lynch {
            Prompt::HunterFinalShotLynched
        } else {
            Prompt::HunterFinalShotKilled
        };
        let options: Vec<PlayerId> = alive_ids.iter().copied().filter(|&id| id != hunter_id).collect();
        let lynched = method == KillMethod::Lynch;
        // Skip (an empty pick) and no answer are told apart
        // (Werewolf.cs:5453-5463).
        match presenter.ask_targets(hunter_id, prompt, &options, 1).await {
            Some(picked) if picked.is_empty() => {
                let announcement = Announcement::HunterSkipChoice {
                    hunter: hunter_id,
                    lynched,
                };
                presenter.narrate(NarrationEvent::Announce(announcement)).await;
            }
            picked => match picked.and_then(|p| validate_pick(p, &options, 1)) {
                Some(target) => shots.push((target[0], hunter_id)),
                None => {
                    let announcement = Announcement::HunterNoChoice {
                        hunter: hunter_id,
                        lynched,
                    };
                    presenter.narrate(NarrationEvent::Announce(announcement)).await;
                }
            },
        }
    }

    shots
}

/// Werewolf.cs's generic per-kill lover check, embedded in `KillPlayer`
/// itself (Werewolf.cs:5609-5616, `KillLover` at 5706-5715): whenever an
/// in-love player dies, their partner dies too (`KillMethod::LoverDied`),
/// unless the partner is already dying in this same batch — a mass-death
/// event (an Arsonist's burn catching both lovers at once, say) needs no
/// extra chained death, matching the legacy `!(dyingSimultaneously?
/// .Contains(x) ?? false)` guard.
///
/// Needs no question asked, unlike `resolve_hunter_shots` — this is
/// unconditional. And unlike a Hunter's shot (which could in principle
/// keep triggering more Hunters), this can't chain past one extra death:
/// Cupid links exactly one pair, so a lover's own death has no lover of
/// their own to chain into.
pub fn resolve_lover_deaths(
    new_deaths: &[(PlayerId, KillMethod)],
    lovers: &LoversState,
    alive: &[AlivePlayer],
) -> Vec<(PlayerId, KillMethod)> {
    let mut chain: Vec<(PlayerId, KillMethod)> = vec![];

    for &(victim, _) in new_deaths {
        let Some(partner) = lovers.partner_of(victim) else {
            continue;
        };
        let partner_alive = alive.iter().any(|p| p.id == partner);
        let already_dying = new_deaths.iter().any(|&(v, _)| v == partner) || chain.iter().any(|&(v, _)| v == partner);
        if partner_alive && !already_dying {
            chain.push((partner, KillMethod::LoverDied));
        }
    }

    chain
}

/// The percent chance a Grave Digger's trap catches whoever visits them
/// (Werewolf.cs:2389: `20 + (30 - 30 * 0.5^(dug-1))`, halved for a
/// Village-team visitor at 2390). Only called with `dug_graves >= 1` —
/// the real code short-circuits to a guaranteed-safe visit otherwise
/// (2380: `if (visited.DugGravesLastNight < 1) return VisitResult.Success`),
/// modeled by the caller never invoking this at all rather than this
/// function handling a `0` specially.
///
/// C#'s `Program.R.Next(100) < fallChance` compares an integer roll
/// against a `double`, so a fractional `fallChance` (e.g. `45.5`) is
/// meaningfully more generous than either whole neighbor. `RngSource`
/// only takes an integer percent, so this rounds to the nearest whole
/// percent rather than truncating — closer to the real odds than always
/// rounding down, and simple enough not to need its own `RngSource`
/// method just for one formula.
pub(crate) fn grave_digger_fall_chance_percent(dug_graves: u32, visitor_is_village: bool) -> u32 {
    let chance = 20.0 + (30.0 - 30.0 * 0.5_f64.powi(dug_graves as i32 - 1));
    let chance = if visitor_is_village { chance / 2.0 } else { chance };
    chance.round() as u32
}

/// Spumpkin's day-phase detonation (Werewolf.cs:2901-2930): a 40% chance
/// (Werewolf.cs:2907, no named `Settings` constant — a bare literal in
/// the source itself) of killing both the target (`KillMthd.Shoot` — yes,
/// the same method Gunner's shot uses, confirmed directly:
/// `KillPlayer(check, KillMthd.Shoot, killer: spumpkin, ...)`) and the
/// Spumpkin themself.
///
/// The real code passes a `null` `killMethod` for the Spumpkin's own
/// death (`KillPlayer(spumpkin, killMethod: null, killer: null, ...)`),
/// which in the source only affects whether a DB kill row gets written
/// and whether a Hunter final shot could trigger — irrelevant here (the
/// engine writes no kill rows, and a Spumpkin isn't a Hunter), so both deaths are given the same `KillMethod::Shoot` rather
/// than inventing a "no method" case this crate's `(PlayerId, KillMethod)`
/// death shape has no room for.
///
/// The WiseElder-demotion special case (Werewolf.cs:2916: detonating a
/// Wise Elder transforms the *Spumpkin* to Villager first) doesn't change
/// who dies — `KillPlayer(spumpkin, ...)` and `KillPlayer(check, ...)`
/// both still run unconditionally right after, regardless of which
/// branch of that `switch` fired — so it is achievement bookkeeping
/// that does not change who dies.
pub fn resolve_spumpkin_detonation(day_actions: &[DayAction], rng: &mut dyn RngSource) -> Vec<(PlayerId, KillMethod)> {
    day_actions
        .iter()
        .filter_map(|a| match a {
            DayAction::Detonate { spumpkin, target } if rng.chance(40) => {
                Some([(*spumpkin, KillMethod::Shoot), (*target, KillMethod::Shoot)])
            }
            _ => None,
        })
        .flatten()
        .collect()
}

#[cfg(test)]
mod hunter_shot_tests {
    use super::*;
    use async_trait::async_trait;

    /// Records the prompt/options it was asked, and answers with a fixed
    /// target (or declines if `answer` is `None`) — enough to prove both
    /// *which* question got asked and that the answer becomes a shot.
    struct ScriptedHunterPresenter {
        answer: Option<PlayerId>,
        asked: Vec<(PlayerId, Prompt, Vec<PlayerId>)>,
    }

    #[async_trait(?Send)]
    impl Presenter for ScriptedHunterPresenter {
        async fn ask_targets(
            &mut self,
            player: PlayerId,
            prompt: Prompt,
            options: &[PlayerId],
            count: usize,
        ) -> Option<Vec<PlayerId>> {
            self.asked.push((player, prompt, options.to_vec()));
            if count != 1 {
                return None;
            }
            self.answer.map(|t| vec![t])
        }
    }

    async fn shots(
        new_deaths: &[(PlayerId, KillMethod)],
        died_with_roles: &[(PlayerId, Role)],
        presenter: &mut ScriptedHunterPresenter,
    ) -> Vec<(PlayerId, KillMethod)> {
        resolve_hunter_shots(new_deaths, died_with_roles, &alive_roster(), presenter)
            .await
            .into_iter()
            .map(|(target, _)| (target, KillMethod::HunterShot))
            .collect()
    }

    fn alive_roster() -> Vec<AlivePlayer> {
        vec![
            AlivePlayer {
                id: PlayerId(2),
                role: Role::Villager,
            },
            AlivePlayer {
                id: PlayerId(3),
                role: Role::Seer,
            },
        ]
    }

    #[tokio::test]
    async fn a_lynched_hunter_is_asked_the_lynched_prompt_and_shoots() {
        let hunter = PlayerId(1);
        let target = PlayerId(2);
        let mut presenter = ScriptedHunterPresenter {
            answer: Some(target),
            asked: vec![],
        };
        let new_deaths = [(hunter, KillMethod::Lynch)];
        let died_with_roles = [(hunter, Role::Hunter)];

        let shots = shots(&new_deaths, &died_with_roles, &mut presenter).await;

        assert_eq!(shots, vec![(target, KillMethod::HunterShot)]);
        assert_eq!(presenter.asked.len(), 1);
        assert_eq!(presenter.asked[0].0, hunter);
        assert_eq!(presenter.asked[0].1, Prompt::HunterFinalShotLynched);
        assert!(
            !presenter.asked[0].2.contains(&hunter),
            "the dead hunter should never be offered as their own target"
        );
    }

    #[tokio::test]
    async fn a_hunter_killed_any_other_way_gets_the_killed_prompt() {
        let hunter = PlayerId(1);
        let mut presenter = ScriptedHunterPresenter {
            answer: None,
            asked: vec![],
        };
        let new_deaths = [(hunter, KillMethod::Poison)];
        let died_with_roles = [(hunter, Role::Hunter)];

        shots(&new_deaths, &died_with_roles, &mut presenter).await;

        assert_eq!(presenter.asked[0].1, Prompt::HunterFinalShotKilled);
    }

    /// Werewolf.cs:3419: the default wolf-eat kill explicitly passes
    /// `hunterFinalShot: false` — a plain wolf-eat death never grants a
    /// revenge shot, unlike every other kill method.
    #[tokio::test]
    async fn a_hunter_eaten_by_wolves_gets_no_final_shot_at_all() {
        let hunter = PlayerId(1);
        let mut presenter = ScriptedHunterPresenter {
            answer: Some(PlayerId(2)),
            asked: vec![],
        };
        let new_deaths = [(hunter, KillMethod::Eat)];
        let died_with_roles = [(hunter, Role::Hunter)];

        let shots = shots(&new_deaths, &died_with_roles, &mut presenter).await;

        assert_eq!(shots, vec![]);
        assert!(
            presenter.asked.is_empty(),
            "a wolf-eaten hunter should never be asked for a final shot"
        );
    }

    #[tokio::test]
    async fn declining_the_final_shot_kills_nobody() {
        let hunter = PlayerId(1);
        let mut presenter = ScriptedHunterPresenter {
            answer: None,
            asked: vec![],
        };
        let new_deaths = [(hunter, KillMethod::Lynch)];
        let died_with_roles = [(hunter, Role::Hunter)];

        let shots = shots(&new_deaths, &died_with_roles, &mut presenter).await;

        assert_eq!(shots, vec![]);
    }

    #[tokio::test]
    async fn non_hunters_are_never_asked() {
        let villager = PlayerId(1);
        let mut presenter = ScriptedHunterPresenter {
            answer: Some(PlayerId(2)),
            asked: vec![],
        };
        let new_deaths = [(villager, KillMethod::Lynch)];
        let died_with_roles = [(villager, Role::Villager)];

        let shots = shots(&new_deaths, &died_with_roles, &mut presenter).await;

        assert_eq!(shots, vec![]);
        assert!(
            presenter.asked.is_empty(),
            "a non-Hunter death should never trigger a final-shot question"
        );
    }
}

#[cfg(test)]
mod lover_death_tests {
    use super::*;

    fn alive_pair() -> Vec<AlivePlayer> {
        vec![
            AlivePlayer {
                id: PlayerId(2),
                role: Role::Villager,
            },
            AlivePlayer {
                id: PlayerId(3),
                role: Role::Seer,
            },
        ]
    }

    #[test]
    fn a_dead_lover_takes_their_living_partner_with_them() {
        let victim = PlayerId(1);
        let partner = PlayerId(2);
        let mut lovers = LoversState::default();
        lovers.link(victim, partner);

        let chain = resolve_lover_deaths(&[(victim, KillMethod::Eat)], &lovers, &alive_pair());

        assert_eq!(chain, vec![(partner, KillMethod::LoverDied)]);
    }

    #[test]
    fn no_link_means_no_chained_death() {
        let victim = PlayerId(1);
        let lovers = LoversState::default();
        let chain = resolve_lover_deaths(&[(victim, KillMethod::Eat)], &lovers, &alive_pair());
        assert_eq!(chain, vec![]);
    }

    #[test]
    fn a_partner_already_dead_does_not_die_again() {
        let victim = PlayerId(1);
        let partner = PlayerId(99); // not in alive_pair()
        let mut lovers = LoversState::default();
        lovers.link(victim, partner);

        let chain = resolve_lover_deaths(&[(victim, KillMethod::Eat)], &lovers, &alive_pair());

        assert_eq!(chain, vec![], "a partner who's already dead can't die again");
    }

    #[test]
    fn lovers_dying_simultaneously_do_not_double_chain() {
        let victim = PlayerId(1);
        let partner = PlayerId(2);
        let mut lovers = LoversState::default();
        lovers.link(victim, partner);

        // Both already in the same batch (e.g. an Arsonist's mass burn) -
        // no extra death should be added for either.
        let chain = resolve_lover_deaths(
            &[(victim, KillMethod::Burn), (partner, KillMethod::Burn)],
            &lovers,
            &alive_pair(),
        );

        assert_eq!(chain, vec![]);
    }
}

#[cfg(test)]
mod spumpkin_detonation_tests {
    use super::*;
    use crate::rng::ScriptedRngSource;

    #[test]
    fn a_successful_detonation_kills_both() {
        let spumpkin = PlayerId(1);
        let target = PlayerId(2);
        let actions = [DayAction::Detonate { spumpkin, target }];
        let mut rng = ScriptedRngSource::new().with_chance(true);

        let deaths = resolve_spumpkin_detonation(&actions, &mut rng);

        assert_eq!(deaths, vec![(spumpkin, KillMethod::Shoot), (target, KillMethod::Shoot)]);
    }

    #[test]
    fn a_failed_detonation_kills_nobody() {
        let spumpkin = PlayerId(1);
        let target = PlayerId(2);
        let actions = [DayAction::Detonate { spumpkin, target }];
        let mut rng = ScriptedRngSource::new().with_chance(false);

        assert_eq!(resolve_spumpkin_detonation(&actions, &mut rng), vec![]);
    }

    #[test]
    fn no_detonate_action_means_no_roll_at_all() {
        let mut rng = ScriptedRngSource::new(); // panics if consulted
        assert_eq!(resolve_spumpkin_detonation(&[], &mut rng), vec![]);
    }
}

#[cfg(test)]
mod grave_digger_fall_tests {
    use super::*;

    #[test]
    fn fall_chance_matches_the_real_formula_at_one_dig() {
        // 20 + (30 - 30*0.5^0) = 20, halved for a Village-team visitor -> 10
        assert_eq!(grave_digger_fall_chance_percent(1, true), 10);
    }

    #[test]
    fn fall_chance_matches_the_real_formula_at_two_digs() {
        // 20 + (30 - 30*0.5^1) = 35, halved -> 17.5, rounded -> 18
        assert_eq!(grave_digger_fall_chance_percent(2, true), 18);
    }

    #[test]
    fn fall_chance_is_not_halved_for_a_non_village_visitor() {
        assert_eq!(grave_digger_fall_chance_percent(1, false), 20);
    }

    #[test]
    fn fall_chance_approaches_fifty_percent_for_many_digs() {
        let chance = grave_digger_fall_chance_percent(20, false);
        assert!((49..=50).contains(&chance), "expected ~50, got {chance}");
    }
}
