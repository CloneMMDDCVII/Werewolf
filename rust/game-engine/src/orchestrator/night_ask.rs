//! Asking the night's questions and turning the answers into
//! `NightAction`s.

use super::batch::{ask_phase, single_target_answer, targets_answer, toggle_answer, BatchBuilder};
use super::{AlivePlayer, Answer, NarrationEvent, OnAnswer, Phase, Presenter, Prompt, Question, Quiet};
use crate::messages::{Message, MessageArg};
use crate::roles::definition::NightQuestion;
use crate::roles::{def, NightAction, NightContext, PlayerId, RoleState};
use shared::Role;
use std::collections::HashMap;

/// Tells a wolf's or a Cultist's teammates whom they voted for, as each
/// vote lands (Werewolf.cs:1030-1046 `WolfVotedKill`, 1112-1122
/// `CultistVotedConvert`).
pub struct TeamVotes {
    /// Per question: the voter, who hears their vote, and the key.
    asked: HashMap<usize, (PlayerId, Vec<PlayerId>, &'static str)>,
    options: Vec<Vec<PlayerId>>,
}

impl TeamVotes {
    pub fn for_night(questions: &[Question], players: &[AlivePlayer]) -> Self {
        let team = |voter: PlayerId, of: fn(Role) -> bool| -> Vec<PlayerId> {
            players
                .iter()
                .filter(|p| p.id != voter && of(p.role))
                .map(|p| p.id)
                .collect()
        };
        let mut asked = HashMap::new();
        let mut options = vec![];
        for (index, q) in questions.iter().enumerate() {
            let Question::Targets {
                player,
                prompt,
                options: offered,
                ..
            } = q
            else {
                options.push(vec![]);
                continue;
            };
            options.push(offered.clone());
            let heard = match prompt {
                Prompt::WolfEat | Prompt::WolfCubBonusKill => {
                    Some((team(*player, Role::is_eating_wolf), "WolfVotedKill"))
                }
                Prompt::CultistConvert => Some((team(*player, |r| r == Role::Cultist), "CultistVotedConvert")),
                _ => None,
            };
            if let Some((to, key)) = heard {
                asked.insert(index, (*player, to, key));
            }
        }
        TeamVotes { asked, options }
    }
}

impl OnAnswer for TeamVotes {
    fn answered(&mut self, index: usize, answer: &Answer) -> Vec<NarrationEvent> {
        let (Some((voter, to, key)), Answer::Targets(Some(picked))) = (self.asked.get(&index), answer) else {
            return vec![];
        };
        let [target] = picked.as_slice() else { return vec![] };
        if !self.options[index].contains(target) {
            return vec![];
        }
        let message = Message::new(key, vec![MessageArg::Player(*voter), MessageArg::Player(*target)]);
        to.iter()
            .map(|&to| NarrationEvent::Tell {
                to,
                message: message.clone(),
            })
            .collect()
    }
}

/// Who a night menu offers (Werewolf.cs:5157-5212): every other living
/// player, less the wolves and the Snow Wolf for a wolf, and less the
/// cult for a Cultist.
fn night_options(player: &AlivePlayer, players: &[AlivePlayer]) -> Vec<PlayerId> {
    let eats = Role::is_eating_wolf;
    players
        .iter()
        .filter(|o| o.id != player.id)
        .filter(|o| !eats(player.role) || !(eats(o.role) || o.role == Role::SnowWolf))
        .filter(|o| player.role != Role::Cultist || o.role != Role::Cultist)
        .map(|o| o.id)
        .collect()
}

/// Asks every night question and turns the answers into `NightAction`s,
/// each paired with the player who took it (so the Snow Wolf's freeze,
/// `freeze::apply_freeze`, can cancel one player's actions). The Snow
/// Wolf's freeze question (`freeze::freeze_question`) goes out in the same
/// batch as everyone else's. Returns the actions, the wolves' target (the
/// legacy count, `night::wolf_vote_winner`), and the freeze picked as
/// `(snow_wolf, target)`. What the actions do is `night::resolve_night_sequence`.
///
/// Roles asked after the wolves (`NightQuestion::AfterWolves`: the Witch,
/// who needs the wolves' target) get a second batch once it is known.
///
/// Players in `skip` get no menu tonight: drunk wolves, and wolves after
/// the Blacksmith's silver (Werewolf.cs:5191, 5316). A Chemist who hasn't
/// brewed brews instead of being asked (Werewolf.cs:5259-5271). The
/// Arsonist is offered only houses not yet `doused`, is shown those that
/// are, and can spark only once there are some (Werewolf.cs:5280-5292).
pub async fn resolve_night_with_freeze(
    players: &[AlivePlayer],
    states: &mut HashMap<PlayerId, RoleState>,
    freeze: Option<(PlayerId, Vec<PlayerId>)>,
    presenter: &mut dyn Presenter,
    skip: &[PlayerId],
    doused: &[PlayerId],
) -> (
    Vec<(PlayerId, NightAction)>,
    Option<PlayerId>,
    Option<(PlayerId, PlayerId)>,
) {
    let alive: Vec<PlayerId> = players.iter().map(|p| p.id).collect();
    let mut actions: Vec<(PlayerId, NightAction)> = vec![];

    // --- Stage 1: roles with no dependencies, all asked at once ---
    struct Asked {
        player: AlivePlayer,
        cupid: Option<usize>,
        target: Option<usize>,
        toggle: Option<usize>,
    }
    let mut batch = BatchBuilder::default();
    let freeze_index = freeze
        .as_ref()
        .map(|(snow_wolf, options)| batch.targets(*snow_wolf, Prompt::SnowWolfFreeze, options, 1));
    let mut asked = vec![];
    for player in players {
        let night = def(player.role).night;
        if night == NightQuestion::AfterWolves || skip.contains(&player.id) {
            continue; // asked in stage 2, or not asked
        }
        let state = states.entry(player.id).or_default();
        if matches!(night, NightQuestion::BrewThenPick(_)) && !state.primary_used {
            state.primary_used = true;
            let note = crate::messages::Note::Night(crate::messages::NightNote::ChemistBrewing);
            presenter.narrate(NarrationEvent::Private { to: player.id, note }).await;
            continue;
        }
        let mut a = Asked {
            player: *player,
            cupid: None,
            target: None,
            toggle: None,
        };
        let options = || night_options(player, players);
        match night {
            NightQuestion::Pair(prompt) => a.cupid = Some(batch.targets(player.id, prompt, &alive, 2)),
            NightQuestion::Pick(prompt) | NightQuestion::BrewThenPick(prompt) => {
                a.target = Some(batch.targets(player.id, prompt, &options(), 1));
            }
            // A role model is picked once (Werewolf.cs:5222-5237).
            NightQuestion::RoleModel(prompt) if !state.primary_used => {
                a.target = Some(batch.targets(player.id, prompt, &options(), 1));
            }
            NightQuestion::DouseOrSpark => {
                let options: Vec<PlayerId> = alive
                    .iter()
                    .copied()
                    .filter(|&id| id != player.id && !doused.contains(&id))
                    .collect();
                let living_doused: Vec<PlayerId> = alive.iter().copied().filter(|id| doused.contains(id)).collect();
                let text = if living_doused.is_empty() {
                    Message::new("AskArsonistNoDoused", vec![])
                } else {
                    Message::new("AskArsonist", vec![]).followed_by(Message::new(
                        "AlreadyDousedList",
                        vec![MessageArg::PlayersAnd(living_doused.clone())],
                    ))
                };
                a.target = Some(batch.targets_with_text(player.id, Prompt::ArsonistDouse, &options, text));
                if !living_doused.is_empty() {
                    a.toggle = Some(batch.toggle(player.id, Prompt::ArsonistSpark));
                }
            }
            NightQuestion::RoleModel(_) | NightQuestion::None | NightQuestion::Freeze | NightQuestion::AfterWolves => {}
        }
        asked.push(a);
    }
    let mut votes = TeamVotes::for_night(&batch.questions, players);
    let answers = ask_phase(presenter, Phase::Night, &batch.questions, &mut votes).await;

    let freeze = freeze
        .zip(single_target_answer(&answers, freeze_index))
        .map(|((snow_wolf, _), target)| (snow_wolf, target));
    for a in &asked {
        let behavior = def(a.player.role).behavior;
        let state = states.entry(a.player.id).or_default();
        let ctx = NightContext {
            alive: &alive,
            self_id: a.player.id,
            chosen_target: single_target_answer(&answers, a.target),
            heal_target: None,
            poison_target: None,
            love_targets: targets_answer(&answers, a.cupid).map(|v| (v[0], v[1])),
            wolf_target: None,
            toggle_choice: toggle_answer(&answers, a.toggle),
        };
        actions.extend(
            behavior
                .night_action(&ctx, state)
                .into_iter()
                .map(|act| (a.player.id, act)),
        );
    }

    let wolf_target = crate::night::wolf_vote_winner(
        players.iter().map(|p| p.id),
        actions.iter().filter_map(|(_, a)| match a {
            NightAction::EatVote { target } => Some(*target),
            _ => None,
        }),
    );

    // --- Stage 2: roles depending on a fact produced above, asked
    // together once that fact is known ---
    let mut batch = BatchBuilder::default();
    let mut asked = vec![];
    for player in players {
        if def(player.role).night != NightQuestion::AfterWolves {
            continue; // already handled in stage 1
        }
        // The Witch is the only stage-2 role: both her potion questions.
        let heal = batch.targets(player.id, Prompt::WitchHeal, &alive, 1);
        let poison = batch.targets(player.id, Prompt::WitchPoison, &alive, 1);
        asked.push((*player, heal, poison));
    }
    let answers = ask_phase(presenter, Phase::Night, &batch.questions, &mut Quiet).await;
    for (player, heal, poison) in asked {
        let behavior = def(player.role).behavior;
        let state = states.entry(player.id).or_default();
        let ctx = NightContext {
            alive: &alive,
            self_id: player.id,
            chosen_target: None,
            heal_target: single_target_answer(&answers, Some(heal)),
            poison_target: single_target_answer(&answers, Some(poison)),
            love_targets: None,
            wolf_target,
            toggle_choice: false,
        };
        actions.extend(behavior.night_action(&ctx, state).into_iter().map(|a| (player.id, a)));
    }

    (actions, wolf_target, freeze)
}
