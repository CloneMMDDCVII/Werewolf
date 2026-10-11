//! A scripted [`Presenter`] for tests, so a test states who answers what
//! instead of writing its own `impl Presenter`.
//!
//! Built for `game-engine`'s own tests and, through the `testing` cargo
//! feature, for other crates' tests:
//!
//! ```toml
//! [dev-dependencies]
//! game-engine = { path = "../game-engine", features = ["testing"] }
//! ```
//!
//! [`Script`] answers each question with the first rule that matches it
//! and nothing otherwise, and records every question and narration event
//! so the test can assert on them afterwards:
//!
//! ```
//! # use game_engine::orchestrator::Prompt;
//! # use game_engine::roles::PlayerId;
//! use game_engine::testing::{Script, When};
//! let (wolf, hunter, victim) = (PlayerId(1), PlayerId(2), PlayerId(3));
//! let script = Script::new()
//!     .pick(When::asked(Prompt::WolfEat).by(wolf).in_round(1), victim)
//!     .pick(Prompt::LynchVote, hunter)
//!     .pick(When::asked(Prompt::HunterFinalShotLynched).by(hunter), wolf);
//! ```
//!
//! A test whose answers depend on what happened earlier in the game, or
//! that checks how the engine treats a malformed answer, still writes its
//! own presenter.

use crate::orchestrator::{AlivePlayer, NarrationEvent, Presenter, Prompt, Question};
use crate::roles::PlayerId;
use async_trait::async_trait;
use shared::Role;
use std::mem::{discriminant, Discriminant};

/// The players of a test game, in order.
pub fn roster(players: &[(PlayerId, Role)]) -> Vec<AlivePlayer> {
    players.iter().map(|&(id, role)| AlivePlayer { id, role }).collect()
}

/// Which questions a rule answers: one prompt, optionally only for one
/// player or in one round. A prompt that carries data
/// (`Prompt::GunnerShoot`'s bullets) matches whatever it carries.
#[derive(Debug, Clone, Copy)]
pub struct When {
    prompt: Discriminant<Prompt>,
    player: Option<PlayerId>,
    round: Option<u32>,
}

impl When {
    pub fn asked(prompt: Prompt) -> Self {
        When {
            prompt: discriminant(&prompt),
            player: None,
            round: None,
        }
    }

    /// Only when `player` is asked.
    pub fn by(self, player: PlayerId) -> Self {
        When {
            player: Some(player),
            ..self
        }
    }

    /// Only in `round`, counting from 1 (see `Presenter::advance_round`).
    pub fn in_round(self, round: u32) -> Self {
        When {
            round: Some(round),
            ..self
        }
    }

    fn matches(&self, player: PlayerId, prompt: Prompt, round: u32) -> bool {
        self.prompt == discriminant(&prompt)
            && self.player.is_none_or(|p| p == player)
            && self.round.is_none_or(|r| r == round)
    }
}

impl From<Prompt> for When {
    fn from(prompt: Prompt) -> Self {
        When::asked(prompt)
    }
}

/// A presenter whose players answer from a fixed list of rules. See the
/// module docs.
#[derive(Debug, Default)]
pub struct Script {
    picks: Vec<(When, Vec<PlayerId>)>,
    yes: Vec<When>,
    flee: Option<PlayerId>,
    kill: bool,
    round: u32,
    /// Every question asked, in order. Questions asked in a batch are
    /// recorded one by one, without their `text`.
    pub asked: Vec<Question>,
    /// Every narration event, in order.
    pub narrated: Vec<NarrationEvent>,
}

impl Script {
    /// Nobody answers anything.
    pub fn new() -> Self {
        Script {
            round: 1,
            ..Default::default()
        }
    }

    /// Questions matching `when` are answered with `target`.
    pub fn pick(self, when: impl Into<When>, target: PlayerId) -> Self {
        self.picks(when, vec![target])
    }

    /// Questions matching `when` are answered with `targets`, for a
    /// question that asks for more than one (Cupid's).
    pub fn picks(mut self, when: impl Into<When>, targets: Vec<PlayerId>) -> Self {
        self.picks.push((when.into(), targets));
        self
    }

    /// Yes/no questions matching `when` are answered yes.
    pub fn yes(mut self, when: impl Into<When>) -> Self {
        self.yes.push(when.into());
        self
    }

    /// `player` flees the first time the game checks.
    pub fn flee(self, player: PlayerId) -> Self {
        Script {
            flee: Some(player),
            ..self
        }
    }

    /// An admin ends the game the first time it checks.
    pub fn kill_game(self) -> Self {
        Script { kill: true, ..self }
    }

    /// How many times `player` was asked `prompt` (matched as in [`When`]).
    pub fn times_asked(&self, player: PlayerId, prompt: Prompt) -> usize {
        let when = When::asked(prompt).by(player);
        self.asked
            .iter()
            .filter(|q| when.matches(q.player(), q.prompt(), 0))
            .count()
    }

    /// Whether the game narrated `player` changing from `from` to `to`.
    pub fn transformed(&self, player: PlayerId, from: Role, to: Role) -> bool {
        self.narrated.contains(&NarrationEvent::Transform { player, from, to })
    }
}

#[async_trait(?Send)]
impl Presenter for Script {
    async fn ask_targets(
        &mut self,
        player: PlayerId,
        prompt: Prompt,
        options: &[PlayerId],
        count: usize,
    ) -> Option<Vec<PlayerId>> {
        self.asked.push(Question::Targets {
            player,
            prompt,
            options: options.to_vec(),
            count,
            text: None,
        });
        let round = self.round;
        self.picks
            .iter()
            .find(|(when, _)| when.matches(player, prompt, round))
            .map(|(_, targets)| targets.clone())
    }

    async fn ask_toggle(&mut self, player: PlayerId, prompt: Prompt) -> bool {
        self.asked.push(Question::Toggle { player, prompt });
        self.yes.iter().any(|when| when.matches(player, prompt, self.round))
    }

    fn advance_round(&mut self) {
        self.round += 1;
    }

    async fn narrate(&mut self, event: NarrationEvent) {
        self.narrated.push(event);
    }

    fn drain_fled_players(&mut self) -> Vec<PlayerId> {
        self.flee.take().into_iter().collect()
    }

    fn game_was_killed(&mut self) -> bool {
        std::mem::take(&mut self.kill)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_first_matching_rule_answers() {
        let (wolf, a, b) = (PlayerId(1), PlayerId(2), PlayerId(3));
        let mut script = Script::new()
            .pick(When::asked(Prompt::WolfEat).in_round(1), a)
            .pick(Prompt::WolfEat, b)
            .pick(When::asked(Prompt::GunnerShoot { bullets: 2 }).by(a), wolf);
        assert_eq!(
            script.ask_targets(wolf, Prompt::WolfEat, &[a, b], 1).await,
            Some(vec![a])
        );
        script.advance_round();
        assert_eq!(
            script.ask_targets(wolf, Prompt::WolfEat, &[a, b], 1).await,
            Some(vec![b])
        );
        let shoot = Prompt::GunnerShoot { bullets: 1 };
        assert_eq!(script.ask_targets(a, shoot, &[wolf], 1).await, Some(vec![wolf]));
        assert_eq!(script.ask_targets(b, shoot, &[wolf], 1).await, None);
        assert_eq!(script.times_asked(wolf, Prompt::WolfEat), 2);
        assert!(!script.ask_toggle(wolf, Prompt::MayorReveal).await);
    }
}
