//! The full loop: night, then day, checking for a winner after each,
//! repeating until one is found or a safety cap is hit. This is the first
//! thing in the crate that ties `orchestrator`'s phase resolution to the
//! `evaluate_winner_with_kills` win-condition logic across *multiple*
//! rounds — until now, every proof (`day_orchestration.rs`,
//! `fixture_day_replay.rs`) checked a single night or a single day in
//! isolation.

use crate::death_text::{death_message, DeathContext};
use crate::freeze::{apply_freeze, freeze_question};
use crate::game_end::{check_game_end, wolves_win_plural, EndContext, GameEnd};
use crate::messages::{Announcement, Message, MessageArg, NightNote, Note};
use crate::night::{resolve_night_sequence, NightInput, NightMemory};
use crate::orchestrator::game_over_message;
use crate::orchestrator::{
    apply_day_results, last_day_veto, resolve_day, resolve_hunter_shots, resolve_lover_deaths, resolve_lynch_vote,
    resolve_night_with_freeze, resolve_spumpkin_detonation, AlivePlayer, Answer, NarrationEvent, Phase, Presenter,
    Prompt, Question, TeamVotes,
};
use crate::reveals::resolve_detective;
use crate::rng::RngSource;
use crate::role_info::{role_info, transform_tells, Cause};
use crate::roles::gunner::bullets_left;
use crate::roles::{def, DayAction, LoversState, NightAction, PlayerId, RoleState};
use crate::rules::GameRules;
use crate::summary::{is_winner, GameSummary, SummaryEntry};
use crate::{KillEvent, WinOutcome};
use shared::{KillMethod, Role, Team};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameOutcome {
    pub winner: WinOutcome,
    pub rounds_played: u32,
    /// Every death, in the order it happened, with the victim's identity —
    /// `KillEvent` (used internally for win-checking) only carries the
    /// victim's role, which isn't enough to compare a simulated game
    /// against a specific historical one death-for-death.
    pub deaths: Vec<(PlayerId, KillMethod)>,
}

/// Runs the game to completion: night, day, check win, repeat. `max_rounds`
/// is a safety cap, not a rule — a night+day round where both the eat and
/// lynch votes tie produces zero deaths, and without a cap that would loop
/// forever. Hitting the cap returns `WinOutcome::Unimplemented` rather than
/// panicking, since "the vote never resolved" is a real (if unlikely)
/// outcome this proof-of-concept doesn't have a tie-breaker rule for.
///
/// Any `WinOutcome` other than `Team(_)` (i.e. `RngDependent`,
/// `InsufficientData`, `Unimplemented`) is treated as "keep playing," which
/// is honest for `evaluate_common`'s genuine "game continues" cases but
/// **not** for the RNG/data-gap cases — a real game would have resolved
/// those (with a coin flip, say) rather than continuing. That's a known
/// divergence from real play, not a hidden one: those cases already return
/// distinct, named variants specifically so a caller can tell them apart
/// from an actual "no winner yet."
pub async fn run_game(
    players: &[AlivePlayer],
    presenter: &mut dyn Presenter,
    rng: &mut dyn RngSource,
    max_rounds: u32,
) -> GameOutcome {
    let pool = default_role_pool();
    run_game_with_role_pool(players, &pool, presenter, rng, max_rounds).await
}

/// Every role that can currently be dealt, once each. Stands in for the
/// balancer's pool when a caller has none (fixtures, tests); a real game
/// should pass the pool from `balancing::balance_with_pool`.
fn default_role_pool() -> Vec<Role> {
    shared::ALL_ROLES.iter().copied().filter(|&r| def(r).dealt).collect()
}

/// `run_game`, with the pool of roles the game was dealt from, which the
/// Augur draws on (see `balancing::balance_with_pool`).
pub async fn run_game_with_role_pool(
    players: &[AlivePlayer],
    role_pool: &[Role],
    presenter: &mut dyn Presenter,
    rng: &mut dyn RngSource,
    max_rounds: u32,
) -> GameOutcome {
    run_game_with_rules(players, role_pool, &GameRules::default(), presenter, rng, max_rounds).await
}

/// `run_game_with_role_pool`, following the group's settings (see
/// `rules::GameRules` and `control::game_rules::rules_for`).
pub async fn run_game_with_rules(
    players: &[AlivePlayer],
    role_pool: &[Role],
    rules: &GameRules,
    presenter: &mut dyn Presenter,
    rng: &mut dyn RngSource,
    max_rounds: u32,
) -> GameOutcome {
    let mut game = Game::new(players, role_pool, rules, presenter, rng);
    game.tell_roles().await;
    for round in 1..=max_rounds {
        game.round = round;
        game.turn_bitten_players().await;
        if let Some(outcome) = game.check_end().await {
            return outcome;
        }
        if std::mem::take(&mut game.sandman_sleeps_tonight) {
            game.sleep_through_night().await;
        } else {
            game.night().await;
            if let Some(outcome) = game.check_end().await {
                return outcome;
            }
            game.tell_bitten().await;
        }
        let veto = game.day().await;
        if let Some(outcome) = game.check_end().await {
            return outcome;
        }
        if veto == Some(DayAction::Trouble) {
            game.second_lynch().await;
            if let Some(outcome) = game.check_end().await {
                return outcome;
            }
        }
        game.presenter.advance_round();
    }
    game.give_up(max_rounds).await
}

/// One running game: what is fixed for the whole game (the starting
/// roster, the role pool, the group's rules), the presenter and dice, and
/// everything that carries from one phase to the next.
struct Game<'a> {
    /// The starting roster, in join order.
    players: &'a [AlivePlayer],
    role_pool: &'a [Role],
    rules: &'a GameRules,
    presenter: &'a mut dyn Presenter,
    rng: &'a mut dyn RngSource,
    round: u32,
    alive: Vec<AlivePlayer>,
    states: HashMap<PlayerId, RoleState>,
    /// The role each victim died as, parallel to `deaths`.
    kills: Vec<KillEvent>,
    deaths: Vec<(PlayerId, KillMethod)>,
    lovers: LoversState,
    /// What one night leaves for the next: kerosene, drunk wolves, the
    /// Serial Killer's stumble, when each cultist joined.
    memory: NightMemory,
    /// Set by a Blacksmith's silver by day: the wolves get no menu the
    /// next night (Werewolf.cs:5191).
    silver_spread_tonight: bool,
    /// A Wolf Cub died: the wolves get a second kill next night
    /// (Werewolf.cs:1047-1053). The legacy flag is reset only at the end
    /// of a night's resolution (Werewolf.cs:3235), after that night's
    /// menus went out (Werewolf.cs:3030), so the second vote is asked in
    /// the same night as the first and both resolve together.
    wolf_cub_bonus_pending: bool,
    /// Set by the Sandman by day: the next night is skipped.
    sandman_sleeps_tonight: bool,
    /// Bitten by the Alpha Wolf last night (Werewolf.cs:3289, `bitten`):
    /// they turn Wolf at the start of this night, before any menus go out
    /// (Werewolf.cs:2988-2998). See `apply_pending_alpha_bites`.
    bitten_pending: Vec<PlayerId>,
    /// How much of `deaths` had already happened the last time a Grave
    /// Digger dug (Werewolf.cs:5294-5299: `DugGravesLastNight =
    /// diedSinceLastGrave.Count`). The rest of `deaths` is
    /// `diedSinceLastGrave`.
    deaths_counted_for_graves: usize,
    /// Whoever the Snow Wolf froze last night, who can't be frozen again
    /// tonight (Werewolf.cs:5276).
    frozen_last_night: Option<PlayerId>,
}

impl<'a> Game<'a> {
    fn new(
        players: &'a [AlivePlayer],
        role_pool: &'a [Role],
        rules: &'a GameRules,
        presenter: &'a mut dyn Presenter,
        rng: &'a mut dyn RngSource,
    ) -> Self {
        Game {
            players,
            role_pool,
            rules,
            presenter,
            rng,
            round: 0,
            alive: players.to_vec(),
            states: HashMap::new(),
            kills: vec![],
            deaths: vec![],
            lovers: LoversState::default(),
            memory: NightMemory::default(),
            silver_spread_tonight: false,
            wolf_cub_bonus_pending: false,
            sandman_sleeps_tonight: false,
            bitten_pending: vec![],
            deaths_counted_for_graves: 0,
            frozen_last_night: None,
        }
    }

    fn reveal(&self) -> bool {
        self.rules.show_roles_on_death
    }

    fn role_of(&self, id: PlayerId) -> Option<Role> {
        self.alive.iter().find(|p| p.id == id).map(|p| p.role)
    }

    async fn narrate(&mut self, event: NarrationEvent) {
        self.presenter.narrate(event).await;
    }

    async fn announce(&mut self, announcement: Announcement) {
        self.presenter.narrate(NarrationEvent::Announce(announcement)).await;
    }

    async fn narrate_all(&mut self, events: Vec<NarrationEvent>) {
        narrate_all(events, &mut *self.presenter).await;
    }

    /// Everyone is told their role, with their team (Werewolf.cs:622, 1612-1641).
    async fn tell_roles(&mut self) {
        for i in 0..self.alive.len() {
            let p = self.alive[i];
            let message = role_info(p.role, &self.alive, self.rules.thief_full);
            self.narrate(NarrationEvent::Tell { to: p.id, message }).await;
        }
    }

    async fn turn_bitten_players(&mut self) {
        apply_pending_alpha_bites(&self.bitten_pending, &mut self.alive, &mut *self.presenter).await;
        self.bitten_pending.clear();
    }

    /// The Sandman's night: skipped, and what it would have used up is
    /// reset (Werewolf.cs:3011-3020).
    async fn sleep_through_night(&mut self) {
        self.wolf_cub_bonus_pending = false;
        self.memory.drunk.clear();
        self.announce(Announcement::SandmanNight).await;
    }

    /// One night: the Grave Digger's graves, every night question in one
    /// batch, the Snow Wolf's freeze, `night::resolve_night_sequence`, then
    /// the deaths and the Hunter shots they trigger.
    async fn night(&mut self) {
        // Werewolf.cs:5295-5296 leaves Flee/Idle deaths out of the count.
        let buried: Vec<PlayerId> = self.deaths[self.deaths_counted_for_graves..]
            .iter()
            .filter(|&&(_, method)| method != KillMethod::Flee && method != KillMethod::Idle)
            .map(|&(id, _)| id)
            .collect();
        let mut dug_graves = buried.len() as u32;
        self.deaths_counted_for_graves = self.deaths.len();

        let seconds = self.rules.night_seconds(self.round, &self.alive);
        self.announce(Announcement::NightTime { seconds }).await;
        // Told as the night's questions go out (Werewolf.cs:5295-5310).
        let graves = match buried.as_slice() {
            [] => Message::new("DigNoGraves", vec![]),
            _ => Message::new("DigGraves", vec![MessageArg::PlayersAnd(buried.clone())]),
        };
        let diggers: Vec<PlayerId> = self
            .alive
            .iter()
            .filter(|p| p.role == Role::GraveDigger)
            .map(|p| p.id)
            .collect();
        for to in diggers {
            self.narrate(NarrationEvent::Tell {
                to,
                message: graves.clone(),
            })
            .await;
        }
        if self.rules.thief_full {
            // A full-mode Thief may try every night (Werewolf.cs:4166).
            for p in self.alive.iter().filter(|p| p.role == Role::Thief) {
                self.states.entry(p.id).or_default().primary_used = false;
            }
        }
        // Drunk wolves get no menu, nor does any wolf after the
        // Blacksmith's silver (Werewolf.cs:5191, 5316).
        let skip: Vec<PlayerId> = self
            .alive
            .iter()
            .filter(|p| p.role.is_eating_wolf() && (self.silver_spread_tonight || self.memory.drunk.contains(&p.id)))
            .map(|p| p.id)
            .collect();
        // The Snow Wolf's freeze goes out with everyone else's night question.
        let freeze_ask = freeze_question(&self.alive, self.frozen_last_night, self.silver_spread_tonight);
        let (mut tagged_actions, _, freeze) = resolve_night_with_freeze(
            &self.alive,
            &mut self.states,
            freeze_ask,
            &mut *self.presenter,
            &skip,
            &self.memory.doused,
        )
        .await;
        if self.round == 1 {
            let told = pick_missing_role_models(&self.alive, &mut self.states, &mut *self.rng);
            self.narrate_all(told).await;
        }
        let wolf_cub_votes = if self.wolf_cub_bonus_pending {
            ask_wolf_cub_votes(&self.alive, &skip, &mut *self.presenter).await
        } else {
            vec![]
        };
        self.wolf_cub_bonus_pending = false;
        let freeze_outcome = apply_freeze(
            freeze,
            &mut tagged_actions,
            &self.alive,
            dug_graves,
            self.rules.thief_full,
            &mut *self.rng,
        );
        self.narrate_all(freeze_outcome.events).await;
        self.frozen_last_night = freeze_outcome.frozen;
        if freeze_outcome.frozen.and_then(|id| self.role_of(id)) == Some(Role::GraveDigger) {
            dug_graves = 0; // tonight's graves were never dug (Werewolf.cs:3163-3168)
        }
        if let Some((snow_wolf, target)) = freeze {
            tagged_actions.push((snow_wolf, NightAction::Freeze { snow_wolf, target }));
        }
        for (_, action) in &tagged_actions {
            if let NightAction::LinkLovers { a, b } = action {
                self.lovers.link(*a, *b);
            }
        }

        let input = NightInput {
            night: self.round,
            actions: &tagged_actions,
            wolf_cub_votes: &wolf_cub_votes,
            frozen: freeze_outcome.frozen,
            saved_from_freeze: freeze_outcome.saved,
            dug_graves,
            thief_full: self.rules.thief_full,
            reveal: self.reveal(),
            role_pool: self.role_pool,
        };
        let night = resolve_night_sequence(
            &input,
            &mut self.alive,
            &mut self.states,
            &mut self.memory,
            &mut *self.rng,
        );
        self.narrate_all(night.events).await;
        self.bitten_pending.extend(&night.bitten);
        let mut night_deaths: Vec<(PlayerId, KillMethod)> = freeze_outcome
            .deaths
            .iter()
            .copied()
            .chain(night.deaths.iter().map(|d| (d.victim, d.method)))
            .collect();
        // Werewolf.cs:4198-4206: the Arsonist's victims are announced first.
        night_deaths.sort_by_key(|&(_, method)| method != KillMethod::Burn);
        let night_by: Vec<(PlayerId, PlayerId)> = night
            .deaths
            .iter()
            .filter_map(|d| d.by.map(|by| (d.victim, by)))
            .collect();
        if night_deaths.is_empty() {
            self.announce(Announcement::NoAttack).await;
        }
        self.wolf_cub_bonus_pending |= self
            .process_deaths_and_hunter_shots(&night_deaths, &night_by, &[])
            .await;
    }

    /// Werewolf.cs:4455-4458: the bitten are told, after the end check.
    async fn tell_bitten(&mut self) {
        let bitten: Vec<PlayerId> = self
            .bitten_pending
            .iter()
            .copied()
            .filter(|id| self.alive.iter().any(|p| p.id == *id))
            .collect();
        for to in bitten {
            self.narrate(NarrationEvent::Private {
                to,
                note: Note::Night(NightNote::PlayerBitten),
            })
            .await;
        }
    }

    /// One day: day abilities and the lynch vote in one batch, then what
    /// they cause. Returns the Pacifist's or Troublemaker's veto, whichever
    /// acted last.
    async fn day(&mut self) -> Option<DayAction> {
        let seconds = self.rules.day_seconds(self.alive.len());
        self.announce(Announcement::DayTime { seconds }).await;
        self.announce(Announcement::Day { number: self.round }).await;
        self.announce(Announcement::LynchTime {
            seconds: self.rules.timers.lynch_seconds,
        })
        .await;
        let (day_actions, lynch) = resolve_day(
            &self.alive,
            &mut self.states,
            self.rules,
            &mut *self.presenter,
            &mut *self.rng,
        )
        .await;
        let veto = last_day_veto(&day_actions);
        let lynch_target_role = lynch.target().and_then(|t| self.role_of(t));
        demote_gunner_if_shot_a_wise_elder(&day_actions, &mut self.alive, &mut *self.presenter).await;
        self.silver_spread_tonight = day_actions.iter().any(|a| matches!(a, DayAction::SpreadSilver));
        self.sandman_sleeps_tonight = day_actions.contains(&DayAction::Sleep);
        let mut day_deaths = apply_day_results(&day_actions, lynch.target(), lynch_target_role, &mut self.states);
        day_deaths.extend(resolve_spumpkin_detonation(&day_actions, &mut *self.rng));
        // A Spumpkin who blows up the Wise Elder dies a Villager
        // (Werewolf.cs:2912-2916), after the detonation's own line.
        let detonated_elder = day_actions.iter().find_map(|a| match *a {
            DayAction::Detonate { spumpkin, target }
                if day_deaths.contains(&(spumpkin, KillMethod::Shoot))
                    && self.role_of(target) == Some(Role::WiseElder) =>
            {
                Some((spumpkin, target))
            }
            _ => None,
        });
        let shot_today: Vec<PlayerId> = day_deaths
            .iter()
            .filter(|&&(_, m)| m != KillMethod::Lynch)
            .map(|&(id, _)| id)
            .collect();
        let snoops = resolve_detective(&day_actions, &self.alive, &shot_today, &mut *self.rng);
        self.narrate_all(snoops).await;
        if veto == Some(DayAction::Pacify) {
            self.announce(Announcement::PacifistNoLynchNow).await;
        } else if let Some(announcement) = lynch.announcement() {
            self.announce(announcement).await;
        }
        if veto != Some(DayAction::Pacify) {
            self.announce_prince(lynch.target(), lynch_target_role, &day_deaths)
                .await;
        }
        let shooters: Vec<(PlayerId, PlayerId)> = day_actions
            .iter()
            .filter_map(|a| match *a {
                DayAction::Shoot { shooter, target } => Some((target, shooter)),
                DayAction::Detonate { spumpkin, target } => Some((target, spumpkin)),
                _ => None,
            })
            .collect();
        // A Gunner's shot was told as it happened (see `resolve_day`).
        let gunned: Vec<PlayerId> = day_actions
            .iter()
            .filter_map(|a| match *a {
                DayAction::Shoot { target, .. } => Some(target),
                _ => None,
            })
            .collect();
        self.wolf_cub_bonus_pending |= self
            .process_deaths_and_hunter_shots(&day_deaths, &shooters, &gunned)
            .await;
        if let Some((spumpkin, elder)) = detonated_elder {
            self.announce(Announcement::DetonatedWiseElder { spumpkin, elder })
                .await;
            self.narrate(NarrationEvent::Transform {
                player: spumpkin,
                from: Role::Spumpkin,
                to: Role::Villager,
            })
            .await;
            if let Some(i) = self.deaths.iter().position(|&(id, _)| id == spumpkin) {
                self.kills[i].victim_role = Role::Villager;
            }
        }
        veto
    }

    /// The Troublemaker's second lynch: the whole vote again, with no
    /// other day actions (Werewolf.cs:2548-2846).
    async fn second_lynch(&mut self) {
        self.announce(Announcement::TroubleDoubleLynchNow).await;
        self.announce(Announcement::LynchTime {
            seconds: self.rules.timers.lynch_seconds,
        })
        .await;
        let second = resolve_lynch_vote(
            &self.alive,
            &self.states,
            self.rules,
            &mut *self.presenter,
            &mut *self.rng,
        )
        .await;
        let second_role = second.target().and_then(|t| self.role_of(t));
        let second_deaths = apply_day_results(&[], second.target(), second_role, &mut self.states);
        if let Some(announcement) = second.announcement() {
            self.announce(announcement).await;
        }
        self.announce_prince(second.target(), second_role, &second_deaths).await;
        self.wolf_cub_bonus_pending |= self.process_deaths_and_hunter_shots(&second_deaths, &[], &[]).await;
    }

    /// The checkpoint after every phase: an admin ending the game
    /// (`/killgame`), then any flees, then the win check. `Some` means the
    /// game is over and has already been narrated as such.
    async fn check_end(&mut self) -> Option<GameOutcome> {
        let winner = if self.presenter.game_was_killed() {
            WinOutcome::Killed
        } else {
            self.apply_fled_players().await;
            if self.lynched_tanner() {
                // Werewolf.cs:2768-2776: lynching the Tanner ends the game.
                WinOutcome::Team(Team::Tanner)
            } else {
                let states = &self.states;
                let lovers = &self.lovers;
                let in_love = |id: PlayerId| lovers.partner_of(id).is_some();
                let bullets = |id: PlayerId| states.get(&id).map(bullets_left).unwrap_or(2);
                let role_model = |id: PlayerId| states.get(&id).and_then(|s| s.remembered_player);
                let ctx = EndContext {
                    alive: &self.alive,
                    in_love: &in_love,
                    bullets: &bullets,
                    role_model: &role_model,
                    bitten: &self.bitten_pending,
                };
                let end = check_game_end(&ctx, &mut *self.rng)?;
                self.play_out_ending(&end).await;
                WinOutcome::Team(end.winner)
            }
        };
        Some(self.finish(winner, self.round).await)
    }

    /// The safety cap was hit: a night and a day where both votes tie kill
    /// nobody, and without a cap that would loop forever.
    async fn give_up(&mut self, rounds: u32) -> GameOutcome {
        let winner =
            WinOutcome::Unimplemented("max_rounds reached without a resolved winner (likely repeated tied votes)");
        let message = game_over_message(&winner, false);
        self.narrate(NarrationEvent::GameOver {
            winner: winner.clone(),
            message,
        })
        .await;
        let summary = self.summary(&winner);
        self.narrate(NarrationEvent::GameSummary(summary)).await;
        GameOutcome {
            winner,
            rounds_played: rounds,
            deaths: self.deaths.clone(),
        }
    }

    /// Names the winner, then sends the end-of-game summary.
    async fn finish(&mut self, winner: WinOutcome, rounds: u32) -> GameOutcome {
        let dead_snow_wolves = self.kills.iter().filter(|k| k.victim_role == Role::SnowWolf).count();
        let message = game_over_message(&winner, wolves_win_plural(&self.alive, dead_snow_wolves));
        self.narrate(NarrationEvent::GameOver {
            winner: winner.clone(),
            message,
        })
        .await;
        let summary = self.summary(&winner);
        self.narrate(NarrationEvent::GameSummary(summary)).await;
        GameOutcome {
            winner,
            rounds_played: rounds,
            deaths: self.deaths.clone(),
        }
    }

    fn lynched_tanner(&self) -> bool {
        self.kills
            .iter()
            .any(|k| k.method == KillMethod::Lynch && k.victim_role == Role::Tanner)
    }

    /// What an ending does before the winner is named: its lines, the deaths
    /// it causes (announced by those lines, so their own line is `None`), and
    /// the cult's last convert.
    async fn play_out_ending(&mut self, end: &GameEnd) {
        for line in &end.lines {
            self.announce(line.clone()).await;
        }
        let died_with_roles = record_deaths(&self.alive, &end.deaths, &mut self.kills, &mut self.deaths);
        let reveal_role = self.reveal();
        for (&(victim, method), &(_, role)) in end.deaths.iter().zip(&died_with_roles) {
            self.narrate(NarrationEvent::Death {
                victim,
                role,
                method,
                reveal_role,
                message: None,
            })
            .await;
        }
        self.alive.retain(|p| !end.deaths.iter().any(|&(v, _)| v == p.id));
        for &id in &end.converted {
            if let Some(from) = self.role_of(id) {
                self.narrate(NarrationEvent::Transform {
                    player: id,
                    from,
                    to: Role::Cultist,
                })
                .await;
                if let Some(p) = self.alive.iter_mut().find(|p| p.id == id) {
                    p.role = Role::Cultist;
                }
            }
        }
    }

    /// Builds the end-of-game summary: survivors first, then the dead in the
    /// order they died, each with their final role (`kills` records the role
    /// each victim died as, in the same order as `deaths`).
    fn summary(&self, winner: &WinOutcome) -> GameSummary {
        let lynched_tanner = self.lynched_tanner();
        let in_love = |id: PlayerId| self.lovers.partner_of(id).is_some();
        let entry = |id: PlayerId, role: Role, alive: bool, fled: bool| SummaryEntry {
            id,
            role,
            alive,
            fled,
            in_love: in_love(id),
            won: is_winner(winner, role, in_love(id), lynched_tanner),
        };
        let mut entries: Vec<SummaryEntry> = self
            .players
            .iter()
            .filter_map(|p| self.alive.iter().find(|a| a.id == p.id))
            .map(|a| entry(a.id, a.role, true, false))
            .collect();
        for (&(id, method), kill) in self.deaths.iter().zip(&self.kills) {
            entries.push(entry(id, kill.victim_role, false, method == KillMethod::Flee));
        }
        GameSummary {
            end_roles: self.rules.end_roles,
            entries,
        }
    }

    /// Processes a batch of deaths, then every Hunter final shot they
    /// trigger, repeatedly: a Hunter shot by a Hunter gets a final shot of
    /// their own, and so on down the chain (Werewolf.cs:5480-5481, the
    /// "Domino" case). `by` pairs each victim with their killer;
    /// `announced` are deaths already told as they happened. Returns
    /// whether a Wolf Cub died anywhere in the chain.
    async fn process_deaths_and_hunter_shots(
        &mut self,
        new_deaths: &[(PlayerId, KillMethod)],
        by: &[(PlayerId, PlayerId)],
        announced: &[PlayerId],
    ) -> bool {
        let mut wolf_cub_died = false;
        let mut batch = new_deaths.to_vec();
        let mut by = by.to_vec();
        let mut announced = announced.to_vec();
        while !batch.is_empty() {
            let (processed, died_with_roles) = self.process_deaths(&batch, &by, &announced).await;
            announced.clear();
            wolf_cub_died |= died_with_roles.iter().any(|&(_, role)| role == Role::WolfCub);
            // Werewolf.cs:5473: a Hunter who shot the Wise Elder, right after
            // the shot is announced.
            for &(target, hunter) in &by {
                if died_with_roles.contains(&(target, Role::WiseElder))
                    && processed.contains(&(target, KillMethod::HunterShot))
                {
                    self.announce(Announcement::HunterKilledWiseElder { hunter, elder: target })
                        .await;
                }
            }
            by = resolve_hunter_shots(&processed, &died_with_roles, &self.alive, &mut *self.presenter).await;
            batch = by.iter().map(|&(target, _)| (target, KillMethod::HunterShot)).collect();
        }
        wolf_cub_died
    }

    /// Records, narrates, removes from `alive`, and resolves any transform
    /// triggered by one batch of deaths: the sequence every source of
    /// deaths needs (night, day, a Hunter's shot, a flee).
    ///
    /// Lovers die with their partner here, for every death whatever its
    /// source, as the legacy check lives in the one `KillPlayer`
    /// (Werewolf.cs:5609-5616).
    ///
    /// Returns the deaths processed (the batch plus any lover who followed)
    /// with each victim's role at death, which the Hunter's shot needs.
    async fn process_deaths(
        &mut self,
        new_deaths: &[(PlayerId, KillMethod)],
        by: &[(PlayerId, PlayerId)],
        announced: &[PlayerId],
    ) -> (Vec<(PlayerId, KillMethod)>, Vec<(PlayerId, Role)>) {
        let lover_chain = resolve_lover_deaths(new_deaths, &self.lovers, &self.alive);
        let mut all_deaths = new_deaths.to_vec();
        all_deaths.extend(lover_chain);

        let died_with_roles = record_deaths(&self.alive, &all_deaths, &mut self.kills, &mut self.deaths);
        self.narrate_deaths(&all_deaths, &died_with_roles, by, announced).await;
        self.alive.retain(|p| !all_deaths.iter().any(|&(v, _)| v == p.id));
        for event in apply_transforms(&mut self.alive, &self.states, &died_with_roles) {
            let tells = tells_for_transform(&event, &self.alive, &self.states, &died_with_roles);
            self.narrate(event).await;
            self.narrate_all(tells).await;
        }
        (all_deaths, died_with_roles)
    }

    /// Tells the presenter about every death from this batch, in order, as
    /// they're resolved. `new_deaths` and `died_with_roles` come from the
    /// same `record_deaths` call in the same order. Called before the
    /// victims leave `alive`, so killers who died in the same batch can
    /// still be looked up.
    async fn narrate_deaths(
        &mut self,
        new_deaths: &[(PlayerId, KillMethod)],
        died_with_roles: &[(PlayerId, Role)],
        by: &[(PlayerId, PlayerId)],
        announced: &[PlayerId],
    ) {
        let reveal_role = self.reveal();
        let pack_size = self.alive.iter().filter(|p| p.role.is_wolf()).count();
        // With roles shown, everyone the Arsonist burned shares one line
        // (Werewolf.cs:4198-4202).
        let burned: Vec<(PlayerId, Role)> = new_deaths
            .iter()
            .zip(died_with_roles)
            .filter(|((_, method), _)| *method == KillMethod::Burn)
            .map(|(_, &victim)| victim)
            .collect();
        let mut burn_line_sent = false;
        for (&(victim, method), &(_, role)) in new_deaths.iter().zip(died_with_roles) {
            if announced.contains(&victim) {
                continue; // told the moment it happened (a Gunner's shot)
            }
            if reveal_role && method == KillMethod::Burn {
                let message =
                    (!burn_line_sent).then(|| Message::new("Burning", vec![MessageArg::Reveals(burned.clone())]));
                burn_line_sent = true;
                self.narrate(NarrationEvent::Death {
                    victim,
                    role,
                    method,
                    reveal_role,
                    message,
                })
                .await;
                continue;
            }
            let killer = match method {
                KillMethod::LoverDied => self.lovers.partner_of(victim),
                _ => by.iter().find(|&&(v, _)| v == victim).map(|&(_, k)| k),
            };
            let killer = killer.map(|k| {
                (
                    k,
                    self.role_of(k).unwrap_or(if method == KillMethod::HunterShot {
                        Role::Hunter
                    } else {
                        Role::Villager
                    }),
                )
            });
            let killer_was_lynched =
                killer.is_some_and(|(k, _)| self.deaths.iter().any(|&(id, m)| id == k && m == KillMethod::Lynch));
            // A Harlot who visited a victim died to whoever killed them.
            let killed_by = match (method, killer) {
                (KillMethod::VisitVictim, Some((host, _))) => {
                    let host_method = new_deaths.iter().find(|&&(id, _)| id == host).map(|&(_, m)| m);
                    Some(if host_method == Some(KillMethod::SerialKilled) {
                        Role::SerialKiller
                    } else {
                        Role::Wolf
                    })
                }
                _ => killer.map(|(_, role)| role),
            };
            let message = death_message(&DeathContext {
                victim,
                role,
                method,
                by: killer,
                killer_was_lynched,
                reveal: reveal_role,
                pack_size,
                killed_by,
            });
            self.narrate(NarrationEvent::Death {
                victim,
                role,
                method,
                reveal_role,
                message,
            })
            .await;
        }
    }

    /// Turns whatever `Presenter::drain_fled_players` reports into deaths
    /// through `process_deaths`, like every other death (Werewolf.cs:848-860:
    /// `RemovePlayer`'s `IsRunning` branch calls `CheckRoleChanges` and
    /// `DBKill(p, p, KillMthd.Flee)`). A fled id that isn't alive (already
    /// dead, or never existed) is dropped.
    async fn apply_fled_players(&mut self) {
        // One player can be reported twice in a window (a `/flee` and an
        // admin's `/smite`, or a smite naming them twice); they still die once.
        let mut seen = HashSet::new();
        let fled_deaths: Vec<(PlayerId, KillMethod)> = self
            .presenter
            .drain_fled_players()
            .into_iter()
            .filter(|id| self.alive.iter().any(|p| p.id == *id) && seen.insert(*id))
            .map(|id| (id, KillMethod::Flee))
            .collect();
        if !fled_deaths.is_empty() {
            self.process_deaths(&fled_deaths, &[], &[]).await;
        }
    }

    /// Werewolf.cs:2745-2748: a Prince who survives their first lynch says so.
    async fn announce_prince(
        &mut self,
        target: Option<PlayerId>,
        target_role: Option<Role>,
        day_deaths: &[(PlayerId, KillMethod)],
    ) {
        if let (Some(prince), Some(Role::Prince)) = (target, target_role) {
            if !day_deaths.contains(&(prince, KillMethod::Lynch)) {
                self.announce(Announcement::PrinceLynched { prince }).await;
            }
        }
    }
}

/// Records a batch of deaths into both the win-check kill history and the
/// caller-facing death list, using each victim's role *at the moment they
/// died*, which may already be a transformed role (a Traitor who turned
/// Wolf in an earlier round). Must be called before the victims leave
/// `still_alive`. Returns the (victim, role-at-death) pairs, which the
/// Doppelganger's transform needs.
fn record_deaths(
    still_alive: &[AlivePlayer],
    new_deaths: &[(PlayerId, KillMethod)],
    kills: &mut Vec<KillEvent>,
    deaths: &mut Vec<(PlayerId, KillMethod)>,
) -> Vec<(PlayerId, Role)> {
    let mut died_with_roles = vec![];
    for &(victim, method) in new_deaths {
        let victim_role = still_alive
            .iter()
            .find(|p| p.id == victim)
            .expect("a death should only ever name a player who was alive to be targeted")
            .role;
        kills.push(KillEvent { victim_role, method });
        died_with_roles.push((victim, victim_role));
    }
    deaths.extend(new_deaths.iter().copied());
    died_with_roles
}

async fn narrate_all(events: Vec<NarrationEvent>, presenter: &mut dyn Presenter) {
    for event in events {
        presenter.narrate(event).await;
    }
}

/// The Wise Elder's defensive quirk (Werewolf.cs:2887-2889): shooting one
/// doesn't save them (they still die from the shot, `KillPlayer` runs
/// unconditionally after the Wise Elder case), but it demotes the
/// *Gunner* to Villager. Checked before `apply_day_results` turns
/// `day_actions` into deaths, while `alive` still has the target's role.
async fn demote_gunner_if_shot_a_wise_elder(
    day_actions: &[DayAction],
    alive: &mut [AlivePlayer],
    presenter: &mut dyn Presenter,
) {
    let shooter = day_actions.iter().find_map(|a| match a {
        DayAction::Shoot { shooter, target } => {
            let target_role = alive.iter().find(|p| p.id == *target).map(|p| p.role);
            (target_role == Some(Role::WiseElder)).then_some(*shooter)
        }
        _ => None,
    });
    let Some(shooter) = shooter else { return };
    if let Some(gunner) = alive.iter_mut().find(|p| p.id == shooter) {
        let from = gunner.role;
        presenter
            .narrate(NarrationEvent::Transform {
                player: gunner.id,
                from,
                to: Role::Villager,
            })
            .await;
        gunner.role = Role::Villager;
    }
}

/// Turns every player bitten last night (Werewolf.cs:2988-2998,
/// `TransformationMethod.AlphaBitten`) into a Wolf, at the very start of
/// this night — before `resolve_night_with_freeze` asks anyone anything, matching the
/// real timing (the transform runs during `NightCycle`'s opening
/// per-player reset loop, before `SendNightActions`). A bitten player who
/// died some other way during the day in between (e.g. lynched) is simply
/// skipped — `Transform` itself no-ops on a dead player for this method
/// (Werewolf.cs:1963-1973 doesn't list `AlphaBitten` among the "works
/// even if dead" exceptions), which falling out of `alive.iter_mut().find`
/// already matches for free. Also skips anyone already wolf-team
/// (Werewolf.cs:2991: `!WolfRoles.Contains(p.PlayerRole) && p.PlayerRole
/// != IRole.SnowWolf`) — `Role::is_wolf`, not a plain `Role::Wolf`
/// check, since a bitten Wolf Cub/Lycan/SnowWolf shouldn't be "converted"
/// into the one specific role `IRole.Wolf` either (today's wolf-eat never
/// targets a wolf-team player in the first place, so this is a belt-and-
/// suspenders match to the source rather than a reachable case).
async fn apply_pending_alpha_bites(
    bitten_pending: &[PlayerId],
    alive: &mut [AlivePlayer],
    presenter: &mut dyn Presenter,
) {
    for &player in bitten_pending {
        let Some(from) = alive.iter().find(|p| p.id == player).map(|p| p.role) else {
            continue;
        };
        if from.is_wolf() {
            continue;
        }
        presenter
            .narrate(NarrationEvent::Transform {
                player,
                from,
                to: Role::Wolf,
            })
            .await;
        if let Some(p) = alive.iter_mut().find(|p| p.id == player) {
            p.role = Role::Wolf;
        }
        // No role info here, so whether the Thief steals every night
        // doesn't matter.
        narrate_all(
            transform_tells(player, from, Role::Wolf, Cause::AlphaBitten, alive, false),
            presenter,
        )
        .await;
    }
}

/// Werewolf.cs:1753-1779: a Wild Child or Doppelganger who didn't pick a
/// role model on the first night is given a random one, and isn't asked
/// again.
fn pick_missing_role_models(
    alive: &[AlivePlayer],
    states: &mut HashMap<PlayerId, RoleState>,
    rng: &mut dyn RngSource,
) -> Vec<NarrationEvent> {
    let mut told = vec![];
    for p in alive
        .iter()
        .filter(|p| matches!(p.role, Role::WildChild | Role::Doppelganger))
    {
        let state = states.entry(p.id).or_default();
        let others: Vec<PlayerId> = alive.iter().map(|o| o.id).filter(|&id| id != p.id).collect();
        if state.remembered_player.is_some() || others.is_empty() {
            continue;
        }
        let target = others[rng.pick_index(others.len())];
        state.remembered_player = Some(target);
        state.primary_used = true;
        told.push(NarrationEvent::Private {
            to: p.id,
            note: Note::RoleModelChosen { target },
        });
    }
    told
}

/// The wolves' second kill after a Wolf Cub died, asked alongside the
/// first (Werewolf.cs:1047-1053, 3249-3260). Only wolves with a menu
/// tonight are asked; their votes are tallied by `night`.
async fn ask_wolf_cub_votes(
    alive: &[AlivePlayer],
    skip: &[PlayerId],
    presenter: &mut dyn Presenter,
) -> Vec<(PlayerId, PlayerId)> {
    let options: Vec<PlayerId> = alive.iter().filter(|p| !p.role.is_wolf()).map(|p| p.id).collect();
    let wolves: Vec<PlayerId> = alive
        .iter()
        .filter(|p| p.role.is_eating_wolf() && !skip.contains(&p.id))
        .map(|p| p.id)
        .collect();
    if wolves.is_empty() {
        return vec![];
    }
    let questions: Vec<Question> = wolves
        .iter()
        .map(|&player| Question::Targets {
            player,
            prompt: Prompt::WolfCubBonusKill,
            options: options.clone(),
            count: 1,
            text: None,
        })
        .collect();
    let mut told = TeamVotes::for_night(&questions, alive);
    let answers = presenter.ask_batch(Phase::Night, questions, &mut told).await;
    wolves
        .into_iter()
        .zip(answers)
        .filter_map(|(wolf, answer)| match answer {
            Answer::Targets(Some(picked)) => match picked.as_slice() {
                [target] if options.contains(target) => Some((wolf, *target)),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// What one of `apply_transforms`' changes tells the player and their
/// teams (see `role_info::transform_tells`).
fn tells_for_transform(
    event: &NarrationEvent,
    alive: &[AlivePlayer],
    states: &HashMap<PlayerId, RoleState>,
    just_died: &[(PlayerId, Role)],
) -> Vec<NarrationEvent> {
    let &NarrationEvent::Transform { player, from, to } = event else {
        return vec![];
    };
    let model = || states.get(&player).and_then(|s| s.remembered_player);
    let cause = match from {
        Role::SnowWolf => Cause::SnowWolfAlone,
        Role::Traitor => Cause::Traitor,
        Role::ApprenticeSeer => Cause::ApprenticeSeer {
            model: just_died
                .iter()
                .find(|&&(_, role)| role == Role::Seer)
                .map(|&(id, _)| id),
        },
        Role::WildChild => match model() {
            Some(model) => Cause::WildChild { model },
            None => return vec![],
        },
        Role::Doppelganger => match model() {
            Some(model) => Cause::Doppelganger { model },
            None => return vec![],
        },
        _ => return vec![],
    };
    // A Doppelganger who copies a Thief is told the plain Thief's role
    // info; deaths don't carry the game's rules this far.
    transform_tells(player, from, to, cause, alive, false)
}

/// Applies role transforms. Most are triggered by the current alive
/// roster's composition rather than by reacting to one specific death
/// event — "no wolf muscle currently alive," "no Seer currently alive,"
/// "role model not in the alive list" are the same check either way, and
/// evaluating them fresh each round is simpler than threading through
/// exactly which death caused it. Doppelganger is the one exception that
/// needs to know more than "did my role model die" — it needs *what they
/// were*, hence `just_died`.
///
/// - Snow Wolf turns Wolf once no other wolf remains alive
///   (Werewolf.cs:4485-4496).
/// - Traitor turns Wolf once no wolf-muscle role remains alive
///   (Werewolf.cs:4499-4512).
/// - Apprentice Seer turns Seer once no Seer remains alive
///   (Werewolf.cs:4053).
/// - Wild Child turns Wolf once her remembered role model (see
///   `roles::wild_child`) is no longer among the alive.
/// - Doppelganger copies whatever role their remembered role model had
///   the moment they died (Werewolf.cs:1936-1937) — not always Wolf,
///   unlike Wild Child.
///
/// Idempotent: once a player's role has been transformed away from the
/// one a given branch matches on, that branch doesn't match them again.
///
/// Returns every transform that happened, as `NarrationEvent`s, for the
/// caller to narrate — same shape as `record_deaths` returning
/// `(PlayerId, Role)` pairs rather than narrating inline: building the
/// event and mutating `player.role` are one step here (the `from` has to
/// be captured before it's overwritten), but *telling* a presenter is a
/// separate concern the caller (`run_game`) owns, same as it does for
/// deaths.
fn apply_transforms(
    alive: &mut [AlivePlayer],
    states: &HashMap<PlayerId, RoleState>,
    just_died: &[(PlayerId, Role)],
) -> Vec<NarrationEvent> {
    let any_wolf_muscle = alive.iter().any(|p| p.role.is_wolf());
    // The legacy `WolfRoles` (Werewolf.cs:48), which leave out the Snow Wolf.
    let any_eating_wolf = alive.iter().any(|p| p.role.is_eating_wolf());
    let any_seer = alive.iter().any(|p| p.role == Role::Seer);
    let alive_ids: Vec<PlayerId> = alive.iter().map(|p| p.id).collect();
    let mut events = vec![];

    for player in alive.iter_mut() {
        match player.role {
            // The last wolf standing stops freezing and starts eating
            // (Werewolf.cs:4485-4496). Checked before the Traitor, who
            // only turns when no Snow Wolf is left either.
            Role::SnowWolf if !any_eating_wolf => {
                events.push(NarrationEvent::Transform {
                    player: player.id,
                    from: player.role,
                    to: Role::Wolf,
                });
                player.role = Role::Wolf;
            }
            Role::Traitor if !any_wolf_muscle => {
                events.push(NarrationEvent::Transform {
                    player: player.id,
                    from: player.role,
                    to: Role::Wolf,
                });
                player.role = Role::Wolf;
            }
            Role::ApprenticeSeer if !any_seer => {
                events.push(NarrationEvent::Transform {
                    player: player.id,
                    from: player.role,
                    to: Role::Seer,
                });
                player.role = Role::Seer;
            }
            Role::WildChild => {
                if let Some(model) = states.get(&player.id).and_then(|s| s.remembered_player) {
                    if !alive_ids.contains(&model) {
                        events.push(NarrationEvent::Transform {
                            player: player.id,
                            from: player.role,
                            to: Role::Wolf,
                        });
                        player.role = Role::Wolf;
                    }
                }
            }
            Role::Doppelganger => {
                if let Some(model) = states.get(&player.id).and_then(|s| s.remembered_player) {
                    if let Some(&(_, model_role)) = just_died.iter().find(|&&(id, _)| id == model) {
                        events.push(NarrationEvent::Transform {
                            player: player.id,
                            from: player.role,
                            to: model_role,
                        });
                        player.role = model_role;
                    }
                }
            }
            _ => {}
        }
    }
    events
}

#[cfg(test)]
mod alpha_bite_transform_tests {
    use super::*;
    use async_trait::async_trait;

    struct RecordingPresenter {
        transforms: Vec<NarrationEvent>,
    }

    #[async_trait(?Send)]
    impl Presenter for RecordingPresenter {
        async fn ask_targets(
            &mut self,
            _player: PlayerId,
            _prompt: crate::orchestrator::Prompt,
            _options: &[PlayerId],
            _count: usize,
        ) -> Option<Vec<PlayerId>> {
            None
        }

        async fn narrate(&mut self, event: NarrationEvent) {
            self.transforms.push(event);
        }
    }

    #[tokio::test]
    async fn a_bitten_player_turns_wolf_at_the_start_of_the_next_night() {
        let bitten = PlayerId(1);
        let mut alive = vec![AlivePlayer {
            id: bitten,
            role: Role::Seer,
        }];
        let mut presenter = RecordingPresenter { transforms: vec![] };

        apply_pending_alpha_bites(&[bitten], &mut alive, &mut presenter).await;

        assert_eq!(alive[0].role, Role::Wolf);
        assert_eq!(
            presenter.transforms,
            vec![
                NarrationEvent::Transform {
                    player: bitten,
                    from: Role::Seer,
                    to: Role::Wolf
                },
                NarrationEvent::Tell {
                    to: bitten,
                    message: Message::new("BittenTurned", vec![])
                },
            ]
        );
    }

    #[tokio::test]
    async fn a_bitten_player_who_died_in_between_is_skipped() {
        let mut alive: Vec<AlivePlayer> = vec![]; // the bitten player is no longer alive
        let mut presenter = RecordingPresenter { transforms: vec![] };

        apply_pending_alpha_bites(&[PlayerId(1)], &mut alive, &mut presenter).await;

        assert!(presenter.transforms.is_empty());
    }

    #[tokio::test]
    async fn nothing_pending_means_no_transforms() {
        let mut alive = vec![AlivePlayer {
            id: PlayerId(1),
            role: Role::Seer,
        }];
        let mut presenter = RecordingPresenter { transforms: vec![] };

        apply_pending_alpha_bites(&[], &mut alive, &mut presenter).await;

        assert_eq!(alive[0].role, Role::Seer);
        assert!(presenter.transforms.is_empty());
    }
}

#[cfg(test)]
mod fled_player_tests {
    use super::*;
    use async_trait::async_trait;

    struct FleeingPresenter {
        pending_fled: Vec<PlayerId>,
        events: Vec<NarrationEvent>,
    }

    #[async_trait(?Send)]
    impl Presenter for FleeingPresenter {
        async fn ask_targets(
            &mut self,
            _player: PlayerId,
            _prompt: crate::orchestrator::Prompt,
            _options: &[PlayerId],
            _count: usize,
        ) -> Option<Vec<PlayerId>> {
            None
        }

        async fn narrate(&mut self, event: NarrationEvent) {
            self.events.push(event);
        }

        fn drain_fled_players(&mut self) -> Vec<PlayerId> {
            std::mem::take(&mut self.pending_fled)
        }
    }

    /// Runs the flee step of a game with `alive` as the roster, returning
    /// who is left, the deaths, and what was narrated.
    async fn flee(
        alive: &[AlivePlayer],
        fled: Vec<PlayerId>,
        lovers: Option<(PlayerId, PlayerId)>,
    ) -> (Vec<AlivePlayer>, Vec<(PlayerId, KillMethod)>, Vec<NarrationEvent>) {
        let mut presenter = FleeingPresenter {
            pending_fled: fled,
            events: vec![],
        };
        let rules = GameRules::default();
        let mut rng = crate::rng::SeededRngSource::new(0);
        let mut game = Game::new(alive, &[], &rules, &mut presenter, &mut rng);
        if let Some((a, b)) = lovers {
            game.lovers.link(a, b);
        }
        game.apply_fled_players().await;
        let (left, deaths) = (game.alive.clone(), game.deaths.clone());
        drop(game);
        (left, deaths, presenter.events)
    }

    #[tokio::test]
    async fn a_fled_player_dies_by_flee_and_is_removed() {
        let fled = PlayerId(1);
        let alive = [
            AlivePlayer {
                id: fled,
                role: Role::Villager,
            },
            AlivePlayer {
                id: PlayerId(2),
                role: Role::Seer,
            },
        ];

        let (alive, deaths, events) = flee(&alive, vec![fled], None).await;

        assert_eq!(alive.len(), 1);
        assert_eq!(alive[0].id, PlayerId(2));
        assert_eq!(deaths, vec![(fled, KillMethod::Flee)]);
        assert!(events.iter().any(|e| matches!(
            e,
            NarrationEvent::Death { victim, role: Role::Villager, method: KillMethod::Flee, reveal_role: true, message: Some(m) }
                if *victim == fled && m.key == "Flee"
        )));
    }

    #[tokio::test]
    async fn fleeing_a_lover_kills_their_partner_too() {
        let fled = PlayerId(1);
        let partner = PlayerId(2);
        let alive = [
            AlivePlayer {
                id: fled,
                role: Role::Villager,
            },
            AlivePlayer {
                id: partner,
                role: Role::Seer,
            },
        ];

        let (alive, deaths, _) = flee(&alive, vec![fled], Some((fled, partner))).await;

        assert!(alive.is_empty());
        assert!(deaths.contains(&(partner, KillMethod::LoverDied)));
    }

    #[tokio::test]
    async fn no_fled_players_touches_nothing() {
        let alive = [AlivePlayer {
            id: PlayerId(1),
            role: Role::Villager,
        }];

        let (alive, deaths, events) = flee(&alive, vec![], None).await;

        assert_eq!(alive.len(), 1);
        assert!(deaths.is_empty());
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn a_fled_id_thats_already_dead_is_silently_ignored() {
        let alive = [AlivePlayer {
            id: PlayerId(2),
            role: Role::Villager,
        }];

        // PlayerId(1) isn't in `alive` at all - already dead or never existed.
        let (alive, deaths, _) = flee(&alive, vec![PlayerId(1)], None).await;

        assert_eq!(alive.len(), 1);
        assert!(deaths.is_empty());
    }
}

#[cfg(test)]
mod transform_tests {
    use super::*;

    #[test]
    fn traitor_turns_wolf_once_no_wolf_muscle_remains() {
        let mut alive = vec![
            AlivePlayer {
                id: PlayerId(1),
                role: Role::Traitor,
            },
            AlivePlayer {
                id: PlayerId(2),
                role: Role::Villager,
            },
        ];
        let events = apply_transforms(&mut alive, &HashMap::new(), &[]);
        assert_eq!(alive[0].role, Role::Wolf);
        assert_eq!(
            events,
            vec![NarrationEvent::Transform {
                player: PlayerId(1),
                from: Role::Traitor,
                to: Role::Wolf,
            }]
        );
    }

    #[test]
    fn traitor_stays_a_traitor_while_a_wolf_is_still_alive() {
        let mut alive = vec![
            AlivePlayer {
                id: PlayerId(1),
                role: Role::Traitor,
            },
            AlivePlayer {
                id: PlayerId(2),
                role: Role::Wolf,
            },
        ];
        apply_transforms(&mut alive, &HashMap::new(), &[]);
        assert_eq!(alive[0].role, Role::Traitor);
    }

    #[test]
    fn wild_child_turns_wolf_once_her_role_model_is_gone() {
        let wild_child = PlayerId(1);
        let mut alive = vec![AlivePlayer {
            id: wild_child,
            role: Role::WildChild,
        }];
        let mut states = HashMap::new();
        states.insert(
            wild_child,
            RoleState {
                remembered_player: Some(PlayerId(99)), // not in `alive`
                ..Default::default()
            },
        );
        apply_transforms(&mut alive, &states, &[]);
        assert_eq!(alive[0].role, Role::Wolf);
    }

    #[test]
    fn wild_child_stays_herself_while_her_role_model_is_still_alive() {
        let wild_child = PlayerId(1);
        let role_model = PlayerId(2);
        let mut alive = vec![
            AlivePlayer {
                id: wild_child,
                role: Role::WildChild,
            },
            AlivePlayer {
                id: role_model,
                role: Role::Villager,
            },
        ];
        let mut states = HashMap::new();
        states.insert(
            wild_child,
            RoleState {
                remembered_player: Some(role_model),
                ..Default::default()
            },
        );
        apply_transforms(&mut alive, &states, &[]);
        assert_eq!(alive[0].role, Role::WildChild);
    }

    #[test]
    fn apprentice_seer_becomes_seer_once_no_seer_remains() {
        let mut alive = vec![
            AlivePlayer {
                id: PlayerId(1),
                role: Role::ApprenticeSeer,
            },
            AlivePlayer {
                id: PlayerId(2),
                role: Role::Villager,
            },
        ];
        apply_transforms(&mut alive, &HashMap::new(), &[]);
        assert_eq!(alive[0].role, Role::Seer);
    }

    #[test]
    fn apprentice_seer_stays_apprentice_while_the_seer_lives() {
        let mut alive = vec![
            AlivePlayer {
                id: PlayerId(1),
                role: Role::ApprenticeSeer,
            },
            AlivePlayer {
                id: PlayerId(2),
                role: Role::Seer,
            },
        ];
        apply_transforms(&mut alive, &HashMap::new(), &[]);
        assert_eq!(alive[0].role, Role::ApprenticeSeer);
    }

    #[test]
    fn doppelganger_copies_the_dead_role_models_actual_role() {
        let doppelganger = PlayerId(1);
        let role_model = PlayerId(2);
        let mut alive = vec![AlivePlayer {
            id: doppelganger,
            role: Role::Doppelganger,
        }];
        let mut states = HashMap::new();
        states.insert(
            doppelganger,
            RoleState {
                remembered_player: Some(role_model),
                ..Default::default()
            },
        );
        let just_died = [(role_model, Role::SerialKiller)];
        let events = apply_transforms(&mut alive, &states, &just_died);
        assert_eq!(alive[0].role, Role::SerialKiller);
        assert_eq!(
            events,
            vec![NarrationEvent::Transform {
                player: doppelganger,
                from: Role::Doppelganger,
                to: Role::SerialKiller,
            }]
        );
    }

    #[test]
    fn doppelganger_stays_itself_if_someone_else_died() {
        let doppelganger = PlayerId(1);
        let role_model = PlayerId(2);
        let mut alive = vec![AlivePlayer {
            id: doppelganger,
            role: Role::Doppelganger,
        }];
        let mut states = HashMap::new();
        states.insert(
            doppelganger,
            RoleState {
                remembered_player: Some(role_model),
                ..Default::default()
            },
        );
        let just_died = [(PlayerId(3), Role::SerialKiller)]; // not the role model
        apply_transforms(&mut alive, &states, &just_died);
        assert_eq!(alive[0].role, Role::Doppelganger);
    }
}
