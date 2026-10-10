//! Running one game: deal roles, drive `game_engine::run_game` through a
//! Telegram presenter, record the result.
//!
//! The engine's `Presenter` is `?Send`, so each game runs as a task pinned
//! to one thread (see [`crate::spawn`]). Everything that talks to a game
//! from outside (button presses, `/flee`, `/killgame`, `/players`) goes
//! through its [`GameHandle`], which is `Send + Sync`.

use crate::sync::LockExt;
use crate::telegram::{Button, Keyboard, MsgRef, SendError, Telegram};
use crate::text::{bold, escape, Texts};
use async_trait::async_trait;
use control::game_rules::game_start;
use control::group_config::{GroupSettings, Toggle};
use control::lobby::Lobby;
use db::{GameId, GameResult, NewGame, NewGamePlayer, Store};
use game_engine::balancing::balance_with_pool;
use game_engine::game::run_game_with_rules;
use game_engine::messages::{Announcement, Message};
use game_engine::orchestrator::{
    prompt_locale_key, AlivePlayer, Answer, NarrationEvent, OnAnswer, Phase, Presenter, Prompt, Question, Quiet,
};
use game_engine::player_list::{render_player_list, PlayerStatus, RosterEntry};
use game_engine::rng::SeededRngSource;
use game_engine::roles::PlayerId;
use game_engine::summary::GameSummary;
use game_engine::WinOutcome;
use rand::seq::SliceRandom;
use shared::{GameMode, KillMethod, Role};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, Notify};
use tokio::time::{sleep_until, Instant};

/// Werewolf.cs:5444: "hunter gets 30 seconds to choose". Used for every
/// question asked on its own rather than as part of a phase.
const SINGLE_QUESTION_SECONDS: u64 = 30;
/// A night question that depends on another answer (the Witch, after the
/// wolves) gets whatever is left of the night timer, but at least this.
const MIN_FOLLOW_UP_SECONDS: u64 = 15;
/// Safety cap passed to `run_game`; real games end long before.
const MAX_ROUNDS: u32 = 100;

/// A button press routed to a running game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Click {
    pub callback_id: String,
    pub user: i64,
    pub qid: u32,
    pub choice: Choice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Pick(usize),
    Skip,
    Yes,
    No,
}

/// Callback data for an answer button: `q:<game>:<question>:<choice>`.
pub fn callback_data(game: u64, qid: u32, choice: Choice) -> String {
    let c = match choice {
        Choice::Pick(i) => i.to_string(),
        Choice::Skip => "s".into(),
        Choice::Yes => "y".into(),
        Choice::No => "n".into(),
    };
    format!("q:{game}:{qid}:{c}")
}

pub fn parse_callback_data(data: &str) -> Option<(u64, u32, Choice)> {
    let mut parts = data.strip_prefix("q:")?.split(':');
    let game = parts.next()?.parse().ok()?;
    let qid = parts.next()?.parse().ok()?;
    let choice = match parts.next()? {
        "s" => Choice::Skip,
        "y" => Choice::Yes,
        "n" => Choice::No,
        n => Choice::Pick(n.parse().ok()?),
    };
    parts.next().is_none().then_some((game, qid, choice))
}

/// The thread-safe side of a running game.
pub struct GameHandle {
    pub seq: u64,
    pub group_id: i64,
    clicks: mpsc::UnboundedSender<Click>,
    fled: Mutex<Vec<i64>>,
    killed: AtomicBool,
    kill_notify: Notify,
    roster: Mutex<Vec<RosterEntry>>,
    /// This game's `ShowRolesDeath` (Random mode may roll it).
    show_roles_on_death: AtomicBool,
    /// The group's "Shuffle player list".
    shuffle: AtomicBool,
    /// Who died, first death first, for the list's order.
    death_order: Mutex<Vec<PlayerId>>,
    /// The last player list posted in the group, for `/players`.
    last_list: Mutex<Option<MsgRef>>,
    /// When `/players` last answered (10-second throttle,
    /// Werewolf.cs:1419).
    last_players_reply: Mutex<Option<Instant>>,
}

/// What `/players` should do during a game.
pub enum PlayersReply {
    /// Asked again within 10 seconds: say nothing.
    Throttled,
    /// Point at the list already in the chat.
    LatestList(MsgRef),
    /// No list posted yet: send this one.
    List(String),
}

impl GameHandle {
    pub fn new(seq: u64, group_id: i64) -> (Arc<Self>, mpsc::UnboundedReceiver<Click>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let handle = GameHandle {
            seq,
            group_id,
            clicks: tx,
            fled: Mutex::new(vec![]),
            killed: AtomicBool::new(false),
            kill_notify: Notify::new(),
            roster: Mutex::new(vec![]),
            show_roles_on_death: AtomicBool::new(true),
            shuffle: AtomicBool::new(false),
            death_order: Mutex::new(vec![]),
            last_list: Mutex::new(None),
            last_players_reply: Mutex::new(None),
        };
        (Arc::new(handle), rx)
    }

    /// Returns false if the game already ended.
    pub fn click(&self, click: Click) -> bool {
        self.clicks.send(click).is_ok()
    }

    pub fn is_playing(&self, telegram_id: i64) -> bool {
        self.roster
            .locked()
            .iter()
            .any(|p| p.id == PlayerId(telegram_id as u64))
    }

    /// Roles aren't dealt yet: the lobby has closed but the game hasn't
    /// started.
    pub fn is_initializing(&self) -> bool {
        self.roster.locked().is_empty()
    }

    pub fn is_alive(&self, telegram_id: i64) -> bool {
        self.roster
            .locked()
            .iter()
            .any(|p| p.id == PlayerId(telegram_id as u64) && p.alive)
    }

    /// `/flee` mid-game: takes effect at the engine's next checkpoint.
    pub fn flee(&self, telegram_id: i64) {
        let mut fled = self.fled.locked();
        if !fled.contains(&telegram_id) {
            fled.push(telegram_id);
        }
    }

    /// `/killgame` or a forced shutdown: the current wait ends at once and
    /// the engine stops at its next checkpoint.
    pub fn kill(&self) {
        self.killed.store(true, Ordering::SeqCst);
        self.kill_notify.notify_one();
    }

    pub fn is_killed(&self) -> bool {
        self.killed.load(Ordering::SeqCst)
    }

    /// `/players` during a game (`OutputPlayers`, Werewolf.cs:1417-1430).
    pub fn players_reply(&self, texts: &Texts, lang: &str) -> PlayersReply {
        {
            let mut last = self.last_players_reply.locked();
            if last.is_some_and(|t| t.elapsed() < Duration::from_secs(10)) {
                return PlayersReply::Throttled;
            }
            *last = Some(Instant::now());
        }
        match *self.last_list.locked() {
            Some(msg) => PlayersReply::LatestList(msg),
            None => PlayersReply::List(self.player_list(texts, lang)),
        }
    }

    /// The player list (`SendPlayerList`, Werewolf.cs:1352-1404): the dead
    /// in the order they died, then the living, shuffled when the group
    /// asks for it.
    pub fn player_list(&self, texts: &Texts, lang: &str) -> String {
        let roster = self.roster.locked().clone();
        let alive = roster.iter().filter(|p| p.alive).count();
        let mut entries = render_player_list(&roster, self.show_roles_on_death.load(Ordering::SeqCst));
        let order = self.death_order.locked().clone();
        let died_at = |id: PlayerId| order.iter().position(|d| *d == id).unwrap_or(usize::MAX);
        entries.sort_by_key(|e| match e.status {
            PlayerStatus::Alive => usize::MAX,
            PlayerStatus::Dead { .. } => died_at(e.id),
        });
        if self.shuffle.load(Ordering::SeqCst) {
            let first_alive = entries
                .iter()
                .position(|e| e.status == PlayerStatus::Alive)
                .unwrap_or(entries.len());
            entries[first_alive..].shuffle(&mut rand::thread_rng());
        }
        let mut out = format!(
            "{}: {alive}/{}",
            bold(&texts.key(lang, "PlayersAlive", &[])),
            roster.len()
        );
        for entry in entries {
            let line = match entry.status {
                PlayerStatus::Alive => format!("{}: {}", escape(&entry.name), texts.key(lang, "Alive", &[])),
                PlayerStatus::Dead { fled, revealed } => {
                    let status = texts.key(lang, if fled { "RanAway" } else { "Dead" }, &[]);
                    match revealed {
                        Some(r) => format!(
                            "{}: {status} - {}{}",
                            escape(&entry.name),
                            texts.role_name(lang, r.role),
                            if r.was_in_love { " ❤️" } else { "" }
                        ),
                        None => format!("{}: {status}", escape(&entry.name)),
                    }
                }
            };
            out.push('\n');
            out.push_str(&line);
        }
        out
    }

    fn set_roster(&self, roster: Vec<RosterEntry>) {
        *self.roster.locked() = roster;
    }

    fn update_roster(&self, f: impl FnOnce(&mut Vec<RosterEntry>)) {
        f(&mut self.roster.locked());
    }

    fn drain_fled(&self) -> Vec<i64> {
        std::mem::take(&mut *self.fled.locked())
    }
}

/// Everything a game task needs; all `Send`, so it can be handed to the
/// thread the game is pinned to.
pub struct GameSetup {
    pub telegram: Arc<dyn Telegram>,
    pub texts: Arc<Texts>,
    pub store: Store,
    pub settings: GroupSettings,
    pub group_id: i64,
    pub group_name: String,
    pub lobby: Lobby,
    /// `/startgame` or `/startchaos`; the group's settings decide what is
    /// actually played (`control::game_rules::game_start`).
    pub requested_mode: GameMode,
    pub seed: u64,
    pub handle: Arc<GameHandle>,
    pub clicks: mpsc::UnboundedReceiver<Click>,
    /// Called last, from the game's thread, whatever the outcome.
    pub on_finish: Box<dyn FnOnce() + Send>,
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Plays one game to the end. Never panics on Telegram or database
/// errors: those are logged and the game carries on, since a missed
/// message is better than a dead game.
pub async fn play(setup: GameSetup) {
    let GameSetup {
        telegram,
        texts,
        store,
        settings,
        group_id,
        group_name,
        lobby,
        requested_mode,
        seed,
        handle,
        clicks,
        on_finish,
    } = setup;
    let lang = settings.language.clone();
    let mut rng = SeededRngSource::new(seed);

    // What this game plays by: the group's settings, or this game's roll
    // in Random mode (Werewolf.cs:174-211).
    let start = game_start(&settings, requested_mode, &mut rng);
    handle
        .show_roles_on_death
        .store(start.rules.show_roles_on_death, Ordering::SeqCst);
    handle
        .shuffle
        .store(settings.is_on(Toggle::ShufflePlayerList), Ordering::SeqCst);
    let dealt = balance_with_pool(
        lobby.players().len(),
        start.mode == GameMode::Chaos,
        lobby.disabled_roles(),
        start.burning_overkill,
        &mut rng,
    );
    let (roles, pool) = match dealt {
        Ok(d) => d,
        Err(e) => {
            log::error!("group {group_id}: could not deal roles: {e:?}");
            let _ = telegram
                .send(
                    group_id,
                    &texts.get(&lang, "", "Sorry, I couldn't deal roles for this game.", &[]),
                    None,
                )
                .await;
            on_finish();
            return;
        }
    };
    let players: Vec<AlivePlayer> = lobby
        .players()
        .iter()
        .zip(roles)
        .map(|(c, role)| AlivePlayer {
            id: PlayerId(c.telegram_id as u64),
            role,
        })
        .collect();
    let names: HashMap<PlayerId, String> = lobby
        .players()
        .iter()
        .map(|c| (PlayerId(c.telegram_id as u64), c.name.clone()))
        .collect();

    let game_id = match store
        .games()
        .start(&NewGame {
            group_id,
            group_name: group_name.clone(),
            mode: start.mode,
            started_at: unix_now(),
            players: players
                .iter()
                .map(|p| NewGamePlayer {
                    telegram_id: p.id.0 as i64,
                    name: names[&p.id].clone(),
                    role: p.role,
                })
                .collect(),
        })
        .await
    {
        Ok(id) => Some(id),
        Err(e) => {
            log::error!("group {group_id}: could not record game start: {e}");
            None
        }
    };

    handle.set_roster(
        players
            .iter()
            .map(|p| RosterEntry {
                id: p.id,
                name: names[&p.id].clone(),
                role: p.role,
                alive: true,
                death_method: None,
                was_in_love: false,
            })
            .collect(),
    );

    let mut presenter = TelegramPresenter {
        telegram: telegram.clone(),
        texts: texts.clone(),
        store: store.clone(),
        lang: lang.clone(),
        group_id,
        game_id,
        handle: handle.clone(),
        clicks,
        names: names.clone(),
        unreachable: Default::default(),
        day: 1,
        night_seconds: start.rules.timers.night_seconds,
        day_seconds: start.rules.timers.day_seconds,
        lynch_seconds: start.rules.timers.lynch_seconds,
        night_deadline: None,
        pending_lynch_time: None,
        summary: None,
        list_changed: true,
        asked: vec![],
        next_qid: 1,
        started: Instant::now(),
    };

    let outcome = run_game_with_rules(&players, &pool, &start.rules, &mut presenter, &mut rng, MAX_ROUNDS).await;
    presenter.flush_lynch_time().await;

    if let Some(id) = game_id {
        let result = match &outcome.winner {
            WinOutcome::Team(t) => GameResult::Won(*t),
            WinOutcome::Killed => GameResult::Killed,
            WinOutcome::RngDependent(why) | WinOutcome::InsufficientData(why) | WinOutcome::Unimplemented(why) => {
                GameResult::Unresolved(why.to_string())
            }
        };
        // Who survived and who won, from the engine's own end-of-game
        // summary when there is one.
        let finals: Vec<(i64, bool, bool)> = match &presenter.summary {
            Some(summary) => summary
                .entries
                .iter()
                .map(|e| (e.id.0 as i64, e.alive, e.won))
                .collect(),
            None => handle
                .roster
                .locked()
                .iter()
                .map(|e| (e.id.0 as i64, e.alive, false))
                .collect(),
        };
        if let Err(e) = store.games().finish(id, &result, unix_now(), &finals).await {
            log::error!("group {group_id}: could not record game end: {e}");
        }
    }
    on_finish();
}

/// The C#'s one-tap abilities: a single button with this label instead
/// of Yes/No (Werewolf.cs:2287, 2294 and 5327).
fn single_button(prompt: Prompt) -> Option<&'static str> {
    match prompt {
        Prompt::MayorReveal => Some("Reveal"),
        Prompt::PacifistPeace => Some("Peace"),
        Prompt::ArsonistSpark => Some("Spark"),
        _ => None,
    }
}

struct Open {
    /// Position in the batch the engine sent.
    index: usize,
    qid: u32,
    player: PlayerId,
    prompt: Prompt,
    msg: Option<MsgRef>,
    shape: Shape,
    answer: Option<Answer>,
}

/// How long `collect` waits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Until {
    /// The whole timer: the day's role actions stay open all day.
    Timer,
    /// Until every question still wanted is answered. The Mayor's reveal
    /// and the Pacifist's peace can be used any time (Werewolf.cs:898,
    /// "Reveal at any time roles"), so nobody waits on them.
    Answered,
}

/// The day questions that stay open from the discussion through the
/// lynch vote.
fn any_time(prompt: Prompt) -> bool {
    matches!(prompt, Prompt::MayorReveal | Prompt::PacifistPeace)
}

enum Shape {
    Targets {
        options: Vec<PlayerId>,
        count: usize,
        picked: Vec<PlayerId>,
    },
    /// Yes/No, or, for the C#'s one-tap abilities, a single button with
    /// this key's label (not pressing it means no).
    Toggle { single: Option<&'static str> },
}

impl Open {
    fn no_answer(&self) -> Answer {
        match self.shape {
            Shape::Targets { .. } => Answer::Targets(None),
            Shape::Toggle { .. } => Answer::Toggle(false),
        }
    }
}

pub struct TelegramPresenter {
    telegram: Arc<dyn Telegram>,
    texts: Arc<Texts>,
    store: Store,
    lang: String,
    group_id: i64,
    game_id: Option<GameId>,
    handle: Arc<GameHandle>,
    clicks: mpsc::UnboundedReceiver<Click>,
    names: HashMap<PlayerId, String>,
    unreachable: std::collections::HashSet<PlayerId>,
    day: u32,
    /// Tonight's and today's timers, as the engine last announced them.
    night_seconds: u32,
    day_seconds: u32,
    lynch_seconds: u32,
    /// When tonight's questions close; `None` until the night's first
    /// batch goes out.
    night_deadline: Option<Instant>,
    /// The engine announces the lynch timer as the day starts; the group
    /// hears it when the vote actually opens, after the day's discussion
    /// (Werewolf.cs:2835, then `LynchCycle`).
    pending_lynch_time: Option<String>,
    /// When the game started, for the summary's playing time.
    started: Instant,
    summary: Option<GameSummary>,
    /// Someone died or fled since the last posted list.
    list_changed: bool,
    /// Who the current batch asks what, so a wolf or cultist is pointed
    /// only at teammates making the same choice (a wolf who ate the Drunk
    /// isn't asked, Werewolf.cs:5192).
    asked: Vec<(PlayerId, Prompt)>,
    next_qid: u32,
}

impl TelegramPresenter {
    fn name(&self, id: PlayerId) -> String {
        escape(self.names.get(&id).map(String::as_str).unwrap_or("?"))
    }

    fn t(&self, key: &str, args: &[&str]) -> String {
        self.texts.key(&self.lang, key, args)
    }

    fn render(&self, message: &Message) -> String {
        self.texts.render(&self.lang, message, &|id| self.name(id))
    }

    async fn group(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Err(e) = self.telegram.send(self.group_id, text, None).await {
            log::warn!("group {}: send failed: {e}", self.group_id);
        }
    }

    async fn flush_lynch_time(&mut self) {
        if let Some(text) = self.pending_lynch_time.take() {
            self.group(&text).await;
            self.post_player_list().await;
        }
    }

    /// Werewolf.cs:1354: posted as each phase starts, unless nothing
    /// changed (a shuffled list is always posted again).
    async fn post_player_list(&mut self) {
        if !self.list_changed && !self.handle.shuffle.load(Ordering::SeqCst) {
            return;
        }
        let text = self.handle.player_list(&self.texts, &self.lang);
        match self.telegram.send(self.group_id, &text, None).await {
            Ok(msg) => {
                *self.handle.last_list.locked() = Some(msg);
                self.list_changed = false;
            }
            Err(e) => log::warn!("group {}: player list not sent: {e}", self.group_id),
        }
    }

    /// A private message; `None` if the player can't be reached (they're
    /// then treated as not answering for the rest of the game).
    async fn pm(&mut self, player: PlayerId, text: &str, keyboard: Option<Keyboard>) -> Option<MsgRef> {
        if self.unreachable.contains(&player) {
            return None;
        }
        match self.telegram.send(player.0 as i64, text, keyboard).await {
            Ok(m) => Some(m),
            Err(SendError::Unreachable) => {
                log::info!("player {} can't be messaged; treating as not answering", player.0);
                self.unreachable.insert(player);
                let _ = self
                    .store
                    .players()
                    .set_reachable(player.0 as i64, false, unix_now())
                    .await;
                None
            }
            Err(e) => {
                log::warn!("PM to {} failed: {e}", player.0);
                None
            }
        }
    }

    fn question_text(&self, player: PlayerId, prompt: Prompt) -> String {
        let text = self.prompt_text(prompt);
        // Werewolf.cs:5190-5214: wolves and cultists are told who to agree with.
        let team: Option<fn(Role) -> bool> = match prompt {
            Prompt::WolfEat => Some(Role::is_eating_wolf),
            Prompt::CultistConvert => Some(|r| r == Role::Cultist),
            _ => None,
        };
        let Some(team) = team else { return text };
        let teammates: Vec<String> = self
            .handle
            .roster
            .locked()
            .iter()
            .filter(|e| e.alive && e.id != player && team(e.role))
            .filter(|e| self.asked.contains(&(e.id, prompt)))
            .map(|e| escape(&e.name))
            .collect();
        match self.texts.discuss_with(&self.lang, &teammates) {
            d if d.is_empty() => text,
            d => format!("{text}  {d}"),
        }
    }

    fn prompt_text(&self, prompt: Prompt) -> String {
        let fallback = match prompt {
            Prompt::WitchHeal => "Who do you want to heal tonight?",
            Prompt::WitchPoison => "Who do you want to poison tonight?",
            Prompt::SpumpkinDetonate => "Who do you want to detonate on?",
            _ => "Who do you choose?",
        };
        let args: Vec<String> = match prompt {
            Prompt::GunnerShoot { bullets } => vec![bullets.to_string()],
            _ => vec![],
        };
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match prompt_locale_key(prompt) {
            Some(key) => self.texts.get(&self.lang, key, fallback, &args),
            None => self.texts.get(&self.lang, "", fallback, &args),
        }
    }

    fn keyboard(&self, qid: u32, shape: &Shape) -> Keyboard {
        let seq = self.handle.seq;
        match shape {
            Shape::Targets { options, picked, .. } => {
                let mut rows: Keyboard = options
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| !picked.contains(p))
                    .map(|(i, p)| {
                        vec![Button::Callback {
                            text: self.names.get(p).cloned().unwrap_or_default(),
                            data: callback_data(seq, qid, Choice::Pick(i)),
                        }]
                    })
                    .collect();
                rows.push(vec![Button::Callback {
                    text: self.t("Skip", &[]),
                    data: callback_data(seq, qid, Choice::Skip),
                }]);
                rows
            }
            Shape::Toggle { single: Some(key) } => {
                vec![vec![Button::Callback {
                    text: self.t(key, &[]),
                    data: callback_data(seq, qid, Choice::Yes),
                }]]
            }
            Shape::Toggle { single: None } => vec![vec![
                Button::Callback {
                    text: self.t("Yes", &[]),
                    data: callback_data(seq, qid, Choice::Yes),
                },
                Button::Callback {
                    text: self.t("No", &[]),
                    data: callback_data(seq, qid, Choice::No),
                },
            ]],
        }
    }

    /// Sends one question privately. An unreachable player answers
    /// nothing, at once.
    async fn open(&mut self, index: usize, q: Question) -> Open {
        let qid = self.next_qid;
        self.next_qid += 1;
        let (player, prompt, shape, text) = match q {
            Question::Targets {
                player,
                prompt,
                options,
                count,
                text,
            } => {
                // The legacy menus never offered yourself, except Cupid's
                // (self-love is allowed).
                let mut options: Vec<PlayerId> = options
                    .into_iter()
                    .filter(|o| *o != player || prompt == Prompt::CupidLink)
                    .collect();
                if self.handle.shuffle.load(Ordering::SeqCst) {
                    options.shuffle(&mut rand::thread_rng());
                }
                (
                    player,
                    prompt,
                    Shape::Targets {
                        options,
                        count,
                        picked: vec![],
                    },
                    text,
                )
            }
            Question::Toggle { player, prompt } => (
                player,
                prompt,
                Shape::Toggle {
                    single: single_button(prompt),
                },
                None,
            ),
        };
        let keyboard = self.keyboard(qid, &shape);
        let text = match text {
            Some(message) => self.render(&message),
            None => self.question_text(player, prompt),
        };
        let msg = self.pm(player, &text, Some(keyboard)).await;
        let mut open = Open {
            index,
            qid,
            player,
            prompt,
            msg,
            shape,
            answer: None,
        };
        if msg.is_none() {
            open.answer = Some(open.no_answer());
        }
        open
    }

    async fn on_click(&mut self, open: &mut [Open], click: Click, on_answer: &mut dyn OnAnswer) {
        let Some(q) = open
            .iter_mut()
            .find(|q| q.qid == click.qid && q.answer.is_none() && !on_answer.closed(q.index))
        else {
            let closed = self.texts.get(&self.lang, "", "That question is closed.", &[]);
            let _ = self.telegram.answer_callback(&click.callback_id, Some(&closed)).await;
            return;
        };
        if q.player != PlayerId(click.user as u64) {
            let _ = self
                .telegram
                .answer_callback(&click.callback_id, Some(&self.t("DidntAsk", &[])))
                .await;
            return;
        }
        let _ = self.telegram.answer_callback(&click.callback_id, None).await;

        let mut chosen_label = None;
        let mut follow_up = false;
        match (&mut q.shape, click.choice) {
            (Shape::Targets { .. }, Choice::Skip) => {
                // An empty pick: skipped, as opposed to never answered.
                q.answer = Some(Answer::Targets(Some(vec![])));
                chosen_label = Some(self.t("Skip", &[]));
            }
            (Shape::Targets { options, count, picked }, Choice::Pick(i)) => {
                let Some(&target) = options.get(i) else { return };
                if picked.contains(&target) {
                    return;
                }
                picked.push(target);
                if picked.len() == *count {
                    let names: Vec<String> = picked
                        .iter()
                        .map(|p| self.names.get(p).cloned().unwrap_or_default())
                        .collect();
                    chosen_label = Some(escape(&names.join(", ")));
                    q.answer = Some(Answer::Targets(Some(picked.clone())));
                } else {
                    follow_up = true;
                }
            }
            (Shape::Toggle { single }, Choice::Yes) => {
                q.answer = Some(Answer::Toggle(true));
                chosen_label = Some(self.t(single.unwrap_or("Yes"), &[]));
            }
            (Shape::Toggle { single: None }, Choice::No) => {
                q.answer = Some(Answer::Toggle(false));
                chosen_label = Some(self.t("No", &[]));
            }
            _ => return,
        }

        let (index, msg, answer) = (q.index, q.msg, q.answer.clone());
        if follow_up {
            // Cupid's second pick (AskCupid2), with the first pick removed.
            if let Some(msg) = msg {
                let keyboard = self.keyboard(q.qid, &q.shape);
                let _ = self.telegram.edit(msg, &self.t("AskCupid2", &[]), Some(keyboard)).await;
            }
            return;
        }
        if let (Some(label), Some(msg)) = (chosen_label, msg) {
            let text = format!("{} - {label}", self.t("ChoiceAccepted", &[]));
            let _ = self.telegram.edit(msg, &text, None).await;
        }
        // Whatever the engine says the moment this answer lands (a lynch
        // vote, announced while the others are still voting).
        if let Some(answer) = answer {
            for event in on_answer.answered(index, &answer) {
                self.narrate(event).await;
            }
        }
    }

    /// Waits for answers until `until` says so, the deadline passes, or
    /// the game is killed. Questions still open stay open; see `close`.
    async fn collect(&mut self, open: &mut [Open], deadline: Instant, until: Until, on_answer: &mut dyn OnAnswer) {
        loop {
            let waiting = |q: &Open| q.answer.is_none() && !on_answer.closed(q.index) && !any_time(q.prompt);
            if self.handle.is_killed() || (until == Until::Answered && !open.iter().any(waiting)) {
                break;
            }
            tokio::select! {
                _ = sleep_until(deadline) => break,
                _ = self.handle.kill_notify.notified() => break,
                click = self.clicks.recv() => match click {
                    Some(c) => self.on_click(open, c, on_answer).await,
                    None => break,
                },
            }
        }
    }

    /// Ends the open questions `which` picks: they count as "no answer",
    /// and their message says why (time is up, or what closed them, such
    /// as a Pacifist's peace: `LynchPeaceTimeout`, Werewolf.cs:2582).
    async fn close(&mut self, open: &mut [Open], which: impl Fn(&Open) -> bool, on_answer: &dyn OnAnswer) {
        for q in open.iter_mut().filter(|q| q.answer.is_none() && which(q)) {
            q.answer = Some(q.no_answer());
            if let Some(msg) = q.msg {
                let key = if on_answer.closed(q.index) {
                    on_answer.closed_key(q.index)
                } else {
                    "TimesUp"
                };
                let _ = self.telegram.edit(msg, &self.t(key, &[]), None).await;
            }
        }
    }

    /// Sends `questions` (with their batch positions), waits until they're
    /// answered, closes what's left, and returns each answer with its
    /// position.
    async fn ask(
        &mut self,
        questions: Vec<(usize, Question)>,
        deadline: Instant,
        on_answer: &mut dyn OnAnswer,
    ) -> Vec<(usize, Answer)> {
        self.asked = questions.iter().map(|(_, q)| (q.player(), q.prompt())).collect();
        let mut open = vec![];
        for (index, q) in questions {
            open.push(self.open(index, q).await);
        }
        self.collect(&mut open, deadline, Until::Answered, on_answer).await;
        self.close(&mut open, |_| true, on_answer).await;
        open.into_iter().map(|q| (q.index, q.answer.unwrap())).collect()
    }

    async fn ask_one(&mut self, q: Question) -> Answer {
        let deadline = Self::secs(SINGLE_QUESTION_SECONDS);
        self.ask(vec![(0, q)], deadline, &mut Quiet)
            .await
            .pop()
            .map(|(_, a)| a)
            .unwrap_or(Answer::Toggle(false))
    }

    fn secs(n: u64) -> Instant {
        Instant::now() + Duration::from_secs(n)
    }

    async fn record_death(&mut self, victim: PlayerId, method: KillMethod) {
        self.list_changed = true;
        self.handle.death_order.locked().push(victim);
        self.handle.update_roster(|r| {
            if let Some(e) = r.iter_mut().find(|e| e.id == victim) {
                e.alive = false;
                e.death_method = Some(method);
            }
        });
        if let Some(game) = self.game_id {
            let _ = self
                .store
                .games()
                .record_kill(game, victim.0 as i64, None, method, self.day, unix_now())
                .await;
        }
    }
}

#[async_trait(?Send)]
impl Presenter for TelegramPresenter {
    async fn ask_targets(
        &mut self,
        player: PlayerId,
        prompt: Prompt,
        options: &[PlayerId],
        count: usize,
    ) -> Option<Vec<PlayerId>> {
        match self
            .ask_one(Question::Targets {
                player,
                prompt,
                options: options.to_vec(),
                count,
                text: None,
            })
            .await
        {
            Answer::Targets(t) => t,
            Answer::Toggle(_) => None,
        }
    }

    async fn ask_toggle(&mut self, player: PlayerId, prompt: Prompt) -> bool {
        matches!(
            self.ask_one(Question::Toggle { player, prompt }).await,
            Answer::Toggle(true)
        )
    }

    async fn ask_batch(&mut self, phase: Phase, questions: Vec<Question>, on_answer: &mut dyn OnAnswer) -> Vec<Answer> {
        let mut answers: Vec<Option<Answer>> = vec![None; questions.len()];
        let mut fill = |got: Vec<(usize, Answer)>| {
            for (i, a) in got {
                answers[i] = Some(a);
            }
        };
        match phase {
            Phase::Night => {
                // The night's first batch starts the night timer; a
                // follow-up (the Witch, after the wolves) gets what's left
                // of it, or at least a few seconds.
                let deadline = match self.night_deadline {
                    Some(d) => d.max(Self::secs(MIN_FOLLOW_UP_SECONDS)),
                    None => Self::secs(self.night_seconds as u64),
                };
                self.night_deadline = Some(deadline);
                fill(
                    self.ask(questions.iter().cloned().enumerate().collect(), deadline, on_answer)
                        .await,
                );
            }
            Phase::Day => {
                // Discussion first, with the day's role actions open for
                // the whole day timer, then the lynch vote. The Mayor and
                // the Pacifist can still act during the vote.
                let (votes, actions): (Vec<_>, Vec<_>) = questions
                    .iter()
                    .cloned()
                    .enumerate()
                    .partition(|(_, q)| q.prompt() == Prompt::LynchVote);
                let mut open = vec![];
                for (index, q) in actions {
                    open.push(self.open(index, q).await);
                }
                if !open.is_empty() || self.pending_lynch_time.is_some() {
                    let day = Self::secs(self.day_seconds as u64);
                    self.collect(&mut open, day, Until::Timer, on_answer).await;
                }
                self.close(&mut open, |q| !any_time(q.prompt), on_answer).await;
                // Peace made before the vote means there is none
                // (Werewolf.cs:2558-2563).
                let peace = !votes.is_empty() && votes.iter().all(|(index, _)| on_answer.closed(*index));
                if peace {
                    self.pending_lynch_time = None;
                } else if !self.handle.is_killed() {
                    self.flush_lynch_time().await;
                    for (index, q) in votes {
                        open.push(self.open(index, q).await);
                    }
                    let lynch = Self::secs(self.lynch_seconds as u64);
                    self.collect(&mut open, lynch, Until::Answered, on_answer).await;
                }
                self.close(&mut open, |_| true, on_answer).await;
                fill(open.into_iter().map(|q| (q.index, q.answer.unwrap())).collect());
            }
        }
        questions
            .iter()
            .zip(answers)
            .map(|(q, a)| {
                a.unwrap_or(match q {
                    Question::Targets { .. } => Answer::Targets(None),
                    Question::Toggle { .. } => Answer::Toggle(false),
                })
            })
            .collect()
    }

    fn advance_round(&mut self) {
        self.day += 1;
    }

    async fn narrate(&mut self, event: NarrationEvent) {
        if !matches!(event, NarrationEvent::Announce(Announcement::LynchTime { .. })) {
            self.flush_lynch_time().await;
        }
        match event {
            NarrationEvent::Death {
                victim,
                method,
                message,
                ..
            } => {
                self.record_death(victim, method).await;
                if let Some(message) = message {
                    self.group(&self.render(&message)).await;
                }
            }
            NarrationEvent::Transform { player, to, .. } => {
                if let Some(game) = self.game_id {
                    let _ = self.store.games().set_final_role(game, player.0 as i64, to).await;
                }
                self.handle.update_roster(|r| {
                    if let Some(e) = r.iter_mut().find(|e| e.id == player) {
                        e.role = to;
                    }
                });
                // What the player and their team are told comes as `Tell`s.
            }
            NarrationEvent::Tell { to, message } => {
                let text = self.render(&message);
                self.pm(to, &text, None).await;
            }
            NarrationEvent::GameOver { winner, message } => {
                let text = match (&winner, message) {
                    (_, Some(message)) => self.render(&message),
                    (WinOutcome::Killed, None) => {
                        self.texts.get(&self.lang, "", "The game was ended by an admin.", &[])
                    }
                    _ => self.texts.get(&self.lang, "", "The game is over.", &[]),
                };
                self.group(&text).await;
            }
            NarrationEvent::Private { to, note } => {
                let text = self.render(&note.message());
                self.pm(to, &text, None).await;
            }
            NarrationEvent::Announce(announcement) => {
                match announcement {
                    Announcement::NightTime { seconds } => {
                        self.night_seconds = seconds;
                        self.night_deadline = None;
                    }
                    Announcement::DayTime { seconds } => self.day_seconds = seconds,
                    Announcement::LynchTime { seconds } => {
                        self.lynch_seconds = seconds;
                        self.pending_lynch_time = Some(self.render(&announcement.message()));
                        return;
                    }
                    _ => {}
                }
                self.group(&self.render(&announcement.message())).await;
                if matches!(announcement, Announcement::NightTime { .. } | Announcement::Day { .. }) {
                    self.post_player_list().await;
                }
            }
            NarrationEvent::GameSummary(summary) => {
                let text = summary.render(&|key| self.texts.lookup(&self.lang, key), &|id| self.name(id));
                // The playing time ends the summary (Werewolf.cs:4919-4923).
                let played = self.t("EndTime", &[&hh_mm_ss(self.started.elapsed())]);
                self.group(&format!("{text}\n{played}")).await;
                self.summary = Some(summary);
            }
        }
    }

    fn drain_fled_players(&mut self) -> Vec<PlayerId> {
        self.handle
            .drain_fled()
            .into_iter()
            .map(|id| PlayerId(id as u64))
            .collect()
    }

    fn game_was_killed(&mut self) -> bool {
        self.handle.is_killed()
    }
}

/// `hh:mm:ss`, as the legacy summary shows its playing time.
fn hh_mm_ss(played: Duration) -> String {
    let secs = played.as_secs();
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playing_time_reads_like_the_legacy_summary() {
        assert_eq!(hh_mm_ss(Duration::from_secs(3 * 3600 + 7 * 60 + 9)), "03:07:09");
        assert_eq!(hh_mm_ss(Duration::from_secs(59)), "00:00:59");
    }

    #[test]
    fn one_tap_abilities_get_one_button() {
        assert_eq!(single_button(Prompt::MayorReveal), Some("Reveal"));
        assert_eq!(single_button(Prompt::PacifistPeace), Some("Peace"));
        assert_eq!(single_button(Prompt::ArsonistSpark), Some("Spark"));
        assert_eq!(single_button(Prompt::SandmanSleep), None);
        assert_eq!(single_button(Prompt::BlacksmithSilver), None);
    }

    #[test]
    fn callback_data_round_trips_and_fits_telegrams_limit() {
        for c in [Choice::Pick(34), Choice::Skip, Choice::Yes, Choice::No] {
            let d = callback_data(u64::MAX, u32::MAX, c);
            assert!(d.len() <= 64, "{d} is too long for Telegram");
            assert_eq!(parse_callback_data(&d), Some((u64::MAX, u32::MAX, c)));
        }
        assert_eq!(parse_callback_data("q:1:2"), None);
        assert_eq!(parse_callback_data("q:1:2:x"), None);
        assert_eq!(parse_callback_data("j:1"), None);
    }
}
