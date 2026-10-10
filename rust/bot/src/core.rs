//! Command dispatch and each group's lifecycle: no game, joining (a
//! `control::lobby::Lobby` plus a [`JoinTimer`]), running (a
//! [`GameHandle`]). Every command passes the moderation permission gate
//! first (`moderation::gate`), as `UpdateHandler` did in the legacy bot.

use crate::game::{self, parse_callback_data, unix_now, Click, GameHandle, GameSetup, PlayersReply};
use crate::join_timer::{mm_ss, JoinTimer, TimerEvent};
use crate::spawn::GameSpawner;
use crate::sync::LockExt;
use crate::telegram::{Button, Chat, Command, Incoming, MsgRef, Telegram, User};
use crate::text::{bold, escape, Texts};
use control::group_config::{linked_group_name, load_settings, GroupSettings, Toggle};
use control::lobby::{AbortReason, Candidate, ExtendError, JoinError, LeaveError, Lobby, StartOutcome};
use db::Store;
use futures::FutureExt;
use moderation::{CommandMessage, GateDecision, Invocation, Moderation, RestoreCheck, Timestamp};
use shared::GameMode;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::Notify;

/// Decides whether one more game fits on this machine.
pub trait Admission: Send + Sync {
    fn admit(&self, running_games: usize) -> bool;
}

/// `control::capacity::CapacityMonitor`: refuses games before free memory
/// runs out.
pub struct MemoryAdmission(Mutex<control::capacity::CapacityMonitor>);

impl MemoryAdmission {
    pub fn new(per_game_bytes: u64) -> Self {
        MemoryAdmission(Mutex::new(control::capacity::CapacityMonitor::new(per_game_bytes)))
    }
}

impl Admission for MemoryAdmission {
    fn admit(&self, running_games: usize) -> bool {
        matches!(
            self.0.locked().try_admit(running_games),
            control::capacity::AdmissionDecision::Admit
        )
    }
}

/// A fixed cap, for tests.
pub struct MaxGames(pub usize);

impl Admission for MaxGames {
    fn admit(&self, running_games: usize) -> bool {
        running_games < self.0
    }
}

/// How long [`BotCore::announce_interrupted_games`] waits before asking the
/// database a second time.
pub const INTERRUPTED_RETRY_DELAY: Duration = Duration::from_secs(2);

pub struct CoreConfig {
    pub default_language: String,
    pub global_admins: Vec<i64>,
    pub language_admins: Vec<i64>,
    /// Where language files are installed (`/uploadlang`) and reloaded
    /// from.
    pub languages_dir: std::path::PathBuf,
    /// Fixed seed for role dealing and game randomness (tests); `None`
    /// draws a fresh one per game.
    pub seed: Option<u64>,
}

struct LobbyCtl {
    /// `/startgame` or `/startchaos`.
    mode: GameMode,
    lobby: Lobby,
    timer: JoinTimer,
    /// Names that joined since the last "joined in the last 30 seconds".
    /// Joined since the last "have joined" line: (id, name).
    unannounced: Vec<(i64, String)>,
    /// Messages carrying the Join button, cleared when joining ends.
    join_messages: Vec<MsgRef>,
    settings: GroupSettings,
    group_name: String,
}

enum Slot {
    Joining(Arc<Mutex<LobbyCtl>>),
    Running(Arc<GameHandle>),
}

pub struct BotCore {
    pub(crate) telegram: Arc<dyn Telegram>,
    /// Swapped whole when a language admin installs a file; running games
    /// keep the copy they started with.
    texts: std::sync::RwLock<Arc<Texts>>,
    pub(crate) store: Store,
    moderation: tokio::sync::Mutex<Moderation>,
    spawner: Arc<dyn GameSpawner>,
    admission: Box<dyn Admission>,
    pub(crate) config: CoreConfig,
    slots: Mutex<HashMap<i64, Slot>>,
    accepting: AtomicBool,
    next_seq: AtomicU64,
    slot_freed: Notify,
    /// A language admin's checked `/uploadlang` file, until they press
    /// "New" or "Old".
    pub(crate) uploads: Mutex<HashMap<i64, (i18n::LanguageId, std::path::PathBuf)>>,
}

impl BotCore {
    pub async fn new(
        telegram: Arc<dyn Telegram>,
        texts: Arc<Texts>,
        store: Store,
        spawner: Arc<dyn GameSpawner>,
        admission: Box<dyn Admission>,
        config: CoreConfig,
    ) -> Result<Arc<Self>, db::DbError> {
        let moderation = Moderation::load(store.clone(), Timestamp(unix_now())).await?;
        Ok(Arc::new(BotCore {
            telegram,
            texts: std::sync::RwLock::new(texts),
            store,
            moderation: tokio::sync::Mutex::new(moderation),
            spawner,
            admission,
            config,
            slots: Mutex::new(HashMap::new()),
            accepting: AtomicBool::new(true),
            next_seq: AtomicU64::new(1),
            slot_freed: Notify::new(),
            uploads: Mutex::new(HashMap::new()),
        }))
    }

    pub(crate) fn texts(&self) -> Arc<Texts> {
        self.texts.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub(crate) fn set_texts(&self, texts: Arc<Texts>) {
        *self.texts.write().unwrap_or_else(PoisonError::into_inner) = texts;
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Games the last process left unfinished (crash or forced exit): tell
    /// each group, and close the game in the history as interrupted. A
    /// database that fails the first time (still starting, briefly
    /// unreachable) gets one more try after [`INTERRUPTED_RETRY_DELAY`].
    pub async fn announce_interrupted_games(&self) {
        let games = match self.store.games().unfinished().await {
            Ok(g) => g,
            Err(e) => {
                log::warn!(
                    "could not list unfinished games: {e}; retrying in {}s",
                    INTERRUPTED_RETRY_DELAY.as_secs()
                );
                tokio::time::sleep(INTERRUPTED_RETRY_DELAY).await;
                match self.store.games().unfinished().await {
                    Ok(g) => g,
                    Err(e) => {
                        log::error!("could not list unfinished games: {e}");
                        return;
                    }
                }
            }
        };
        for g in games {
            let lang = self.settings(g.group_id).await.language;
            // Bot.cs:493, what the C# said when a node died mid-game.
            let text = self.texts().key(&lang, "NodeShutsDown", &[]);
            let _ = self.telegram.send(g.group_id, &text, None).await;
            if let Err(e) = self.store.games().mark_interrupted(g.id, unix_now()).await {
                log::error!("could not close interrupted game {}: {e}", g.id.0);
            }
        }
    }

    pub fn running_games(&self) -> usize {
        self.slots
            .locked()
            .values()
            .filter(|s| matches!(s, Slot::Running(_)))
            .count()
    }

    pub fn active_groups(&self) -> usize {
        self.slots.locked().len()
    }

    /// First Ctrl+C: no new games; lobbies and games already going finish.
    pub fn stop_accepting(&self) {
        self.accepting.store(false, Ordering::SeqCst);
    }

    /// Resolves once no lobby or game is left.
    pub async fn wait_until_idle(&self) {
        loop {
            let freed = self.slot_freed.notified();
            if self.active_groups() == 0 {
                return;
            }
            freed.await;
        }
    }

    /// Second Ctrl+C: end every lobby and game now, telling each group.
    pub async fn kill_everything(&self) {
        let slots: Vec<(i64, Option<Arc<GameHandle>>)> = {
            let mut map = self.slots.locked();
            let mut out = vec![];
            for (group, slot) in map.iter() {
                match slot {
                    Slot::Joining(ctl) => {
                        let _ = ctl.locked().lobby.kill();
                        out.push((*group, None));
                    }
                    Slot::Running(h) => out.push((*group, Some(h.clone()))),
                }
            }
            map.retain(|_, s| matches!(s, Slot::Running(_)));
            out
        };
        self.slot_freed.notify_waiters();
        for (group, handle) in slots {
            let lang = self.settings(group).await.language;
            let text = self.texts().get(
                &lang,
                "",
                "The bot is shutting down, so this game has been stopped.",
                &[],
            );
            let _ = self.telegram.send(group, &text, None).await;
            if let Some(h) = handle {
                h.kill();
            }
        }
    }

    pub async fn handle(self: &Arc<Self>, incoming: Incoming) {
        match incoming {
            Incoming::Command(cmd) => self.command(cmd).await,
            Incoming::Callback {
                id,
                from,
                message,
                data,
            } => self.callback(id, from, message, data).await,
        }
    }

    pub(crate) async fn settings(&self, group: i64) -> GroupSettings {
        let mut s = match load_settings(&self.store, group).await {
            Ok(s) => s.unwrap_or_default(),
            Err(e) => {
                log::warn!("group {group}: unreadable settings, using defaults: {e}");
                GroupSettings::default()
            }
        };
        if s.language.is_empty() {
            s.language = self.config.default_language.clone();
        }
        s
    }

    pub(crate) async fn say(&self, chat: i64, text: &str) {
        if let Err(e) = self.telegram.send(chat, text, None).await {
            log::warn!("send to {chat} failed: {e}");
        }
    }

    pub(crate) fn is_global_admin(&self, user: i64) -> bool {
        self.config.global_admins.contains(&user)
    }

    fn join_button(&self, group: i64, lang: &str) -> Vec<Vec<Button>> {
        vec![vec![Button::Url {
            text: self.texts().key(lang, "JoinButton", &[]),
            url: format!("https://t.me/{}?start=join{group}", self.telegram.bot_username()),
        }]]
    }

    // ----- dispatch -------------------------------------------------------

    async fn command(self: &Arc<Self>, cmd: Command) {
        let Some(policy) = moderation::policy_for(&cmd.name).or_else(|| crate::menus::policy_for(&cmd.name)) else {
            return; // not one of ours
        };
        let now = Timestamp(unix_now());
        let group_admin = if policy.group_admin_only && cmd.chat.is_group() {
            self.telegram
                .is_group_admin(cmd.chat.id, cmd.from.id)
                .await
                .unwrap_or(false)
        } else {
            false
        };
        let invocation = Invocation {
            sender: cmd.sender,
            chat: if cmd.chat.is_group() {
                moderation::Chat::Group { blocked: None }
            } else {
                moderation::Chat::Private
            },
            sender_is_banned: self.moderation.lock().await.is_banned(cmd.from.id, now),
            sender_is_dev: self.is_global_admin(cmd.from.id),
            sender_is_global_admin: self.is_global_admin(cmd.from.id),
            sender_is_lang_admin: self.config.language_admins.contains(&cmd.from.id),
            sender_is_group_admin: group_admin,
        };
        let lang = if cmd.chat.is_group() {
            self.settings(cmd.chat.id).await.language
        } else {
            self.config.default_language.clone()
        };
        match moderation::check(&policy, &invocation) {
            GateDecision::Run => {}
            GateDecision::Ignore => return,
            GateDecision::Refuse(refusal) => {
                let text = match (refusal.locale_key(), refusal) {
                    (Some(key), _) => self.texts().key(&lang, key, &[]),
                    (None, moderation::Refusal::BlockedChat(chat)) => chat.notice().to_string(),
                    (None, _) => return,
                };
                self.say(cmd.chat.id, &text).await;
                return;
            }
        }

        if cmd.chat.is_group() {
            let _ = self
                .store
                .groups()
                .upsert_name(cmd.chat.id, cmd.chat.title(), now.0)
                .await;
        }

        match cmd.name.as_str() {
            "start" => self.start(&cmd).await,
            "ping" => self.ping(&cmd).await,
            "help" => self.help(&cmd).await,
            "startgame" => self.start_game(&cmd, GameMode::Normal).await,
            "startchaos" => self.start_game(&cmd, GameMode::Chaos).await,
            "join" => self.join_from_group(&cmd).await,
            "flee" => self.flee(&cmd).await,
            "forcestart" => self.force_start(&cmd).await,
            "extend" => self.extend(&cmd).await,
            "players" => self.players(&cmd).await,
            "killgame" => self.kill_game(&cmd).await,
            "smite" => self.smite(&cmd).await,
            "getban" | "permban" | "remban" | "getidles" | "restore" => self.moderation_command(&cmd, &lang).await,
            "config" | "setlang" | "setlink" | "remlink" | "getlang" => self.menu_command(&cmd, &lang).await,
            "validatelangs" => self.validatelangs(&cmd).await,
            "uploadlang" => self.uploadlang(&cmd).await,
            _ => {}
        }
    }

    async fn callback(self: &Arc<Self>, id: String, from: User, message: Option<MsgRef>, data: String) {
        if let Some((seq, qid, choice)) = parse_callback_data(&data) {
            let handle = self.slots.locked().values().find_map(|s| match s {
                Slot::Running(h) if h.seq == seq => Some(h.clone()),
                _ => None,
            });
            let delivered = handle.is_some_and(|h| {
                h.click(Click {
                    callback_id: id.clone(),
                    user: from.id,
                    qid,
                    choice,
                })
            });
            if !delivered {
                let text = self
                    .texts()
                    .get(&self.config.default_language, "", "That game is over.", &[]);
                let _ = self.telegram.answer_callback(&id, Some(&text)).await;
                if let Some(m) = message {
                    let _ = self.telegram.clear_keyboard(m).await;
                }
            }
            return;
        }
        if self.menu_callback(&id, &from, message, &data).await {
            return;
        }
        if self.lang_admin_callback(&id, &from, message, &data).await {
            return;
        }
        if let Some(rest) = data.strip_prefix("restore|") {
            let _ = self.telegram.answer_callback(&id, None).await;
            if !self.is_global_admin(from.id) {
                return;
            }
            let mut parts = rest.split('|');
            let (Some(Ok(old)), Some(Ok(new))) = (parts.next().map(str::parse), parts.next().map(str::parse)) else {
                if let Some(m) = message {
                    let _ = self.telegram.edit(m, "Cancelled.", None).await;
                }
                return;
            };
            let reply = match self.moderation.lock().await.restore(old, new).await {
                Ok(r) => r,
                Err(e) => format!("Restore failed: {e}"),
            };
            match message {
                Some(m) => {
                    let _ = self.telegram.edit(m, &reply, None).await;
                }
                None => self.say(from.id, &reply).await,
            }
            return;
        }
        let _ = self.telegram.answer_callback(&id, None).await;
    }

    // ----- private chat ---------------------------------------------------

    async fn start(self: &Arc<Self>, cmd: &Command) {
        if cmd.chat.is_group() {
            return;
        }
        let now = unix_now();
        let players = self.store.players();
        let _ = players
            .upsert(cmd.from.id, &cmd.from.name, cmd.from.username.as_deref(), now)
            .await;
        let _ = players.set_reachable(cmd.from.id, true, now).await;

        if let Some(group) = cmd.args.strip_prefix("join").and_then(|g| g.parse::<i64>().ok()) {
            // GeneralCommands.cs:322-345: the link works only for members.
            if let Ok(false) = self.telegram.is_group_member(group, cmd.from.id).await {
                let settings = self.settings(group).await;
                let name = self
                    .lobby_of(group)
                    .map(|c| c.locked().group_name.clone())
                    .unwrap_or_default();
                self.say(
                    cmd.from.id,
                    &self
                        .texts()
                        .key(&settings.language, "NotMember", &[&bold(&escape(&name))]),
                )
                .await;
                return;
            }
            self.join(group, &cmd.from).await;
            return;
        }
        let lang = &self.config.default_language;
        let text = self.texts().get(
            lang,
            "",
            "Hi {0}! I'm the Werewolf moderator. Add me to a group and send /startgame there to play. \
             I'll send you your role and night actions here.",
            &[&escape(&cmd.from.name)],
        );
        self.say(cmd.chat.id, &text).await;
    }

    /// GeneralCommands.cs:45-53. Only a bare `/help`: with arguments it was
    /// more likely meant for another bot.
    async fn help(&self, cmd: &Command) {
        if !cmd.args.is_empty() {
            return;
        }
        let text = "<a href=\"https://www.tgwerewolf.com/?referrer=help\">Website</a>\n\
                    <a href=\"https://telegram.me/greywolfsupport\">Telegram Werewolf Support Group</a>\n\
                    <a href=\"https://telegram.me/greywolfdev\">Telegram Werewolf Dev Channel</a>";
        self.say(cmd.chat.id, text).await;
    }

    async fn ping(&self, cmd: &Command) {
        let started = tokio::time::Instant::now();
        let sent = self.telegram.send(cmd.chat.id, "Pong!", None).await;
        let took = format!("{:.3}s", started.elapsed().as_secs_f64());
        if let Ok(msg) = sent {
            let lang = &self.config.default_language;
            let _ = self
                .telegram
                .edit(msg, &self.texts().key(lang, "Ping2", &[&bold(&took)]), None)
                .await;
        }
    }

    // ----- joining --------------------------------------------------------

    async fn start_game(self: &Arc<Self>, cmd: &Command, mode: GameMode) {
        let group = cmd.chat.id;
        let settings = self.settings(group).await;
        let lang = settings.language.clone();
        if !self.accepting.load(Ordering::SeqCst) {
            let text = self.texts().get(
                &lang,
                "",
                "I'm restarting, so I can't start a new game right now. Try again in a minute.",
                &[],
            );
            self.say(group, &text).await;
            return;
        }
        let existing = self.slots.locked().get(&group).map(|s| matches!(s, Slot::Joining(_)));
        match existing {
            Some(true) => {
                let text = self.texts().get(
                    &lang,
                    "",
                    "A game is already being joined here. Press Join to play!",
                    &[],
                );
                let _ = self
                    .telegram
                    .send(group, &text, Some(self.join_button(group, &lang)))
                    .await;
                return;
            }
            Some(false) => {
                let text = self
                    .texts()
                    .get(&lang, "", "A game is already running here. Wait for it to finish.", &[]);
                self.say(group, &text).await;
                return;
            }
            None => {}
        }
        if !self.admission.admit(self.running_games()) {
            let text = self.texts().key(&lang, "NoNodes", &[]);
            self.say(group, &text).await;
            return;
        }

        let lobby = Lobby::from_group_settings(&settings, mode);
        let ctl = Arc::new(Mutex::new(LobbyCtl {
            mode,
            lobby,
            timer: JoinTimer::new(0),
            unannounced: vec![],
            join_messages: vec![],
            settings: settings.clone(),
            group_name: cmd.chat.title().to_string(),
        }));
        {
            let mut slots = self.slots.locked();
            if slots.contains_key(&group) {
                return; // lost a race with another /startgame
            }
            slots.insert(group, Slot::Joining(ctl.clone()));
        }

        let key = if mode == GameMode::Chaos {
            "PlayerStartedChaosGame"
        } else {
            "PlayerStartedGame"
        };
        let text = self.texts().key(&lang, key, &[&escape(&cmd.from.name)]);
        if let Ok(m) = self
            .telegram
            .send(group, &text, Some(self.join_button(group, &lang)))
            .await
        {
            ctl.locked().join_messages.push(m);
        }

        // The starter joins their own game, if the bot can PM them.
        if self.reachable(cmd.from.id).await {
            self.join(group, &cmd.from).await;
        }

        let core = self.clone();
        tokio::spawn(async move { core.run_join_timer(group, ctl).await });
    }

    async fn reachable(&self, user: i64) -> bool {
        matches!(self.store.players().get(user).await, Ok(Some(p)) if p.reachable)
    }

    /// The group whose lobby or game `user` is in, if any.
    fn group_of(&self, user: i64) -> Option<(i64, String)> {
        self.slots.locked().iter().find_map(|(group, slot)| {
            let found = match slot {
                Slot::Joining(ctl) => ctl.locked().lobby.telegram_ids().contains(&user),
                Slot::Running(h) => h.is_alive(user),
            };
            let name = match slot {
                Slot::Joining(ctl) => ctl.locked().group_name.clone(),
                Slot::Running(_) => String::new(),
            };
            found.then_some((*group, name))
        })
    }

    async fn join_from_group(&self, cmd: &Command) {
        let group = cmd.chat.id;
        if !self.reachable(cmd.from.id).await {
            let lang = self.settings(group).await.language;
            let link = format!("@{}", self.telegram.bot_username());
            let text = self.texts().key(&lang, "PMTheBot", &[&escape(&cmd.from.name), &link]);
            let button = vec![vec![Button::Url {
                text: self.texts().key(&lang, "StartMe", &[]),
                url: format!("https://t.me/{}?start=join{group}", self.telegram.bot_username()),
            }]];
            let _ = self.telegram.send(group, &text, Some(button)).await;
            return;
        }
        self.join(group, &cmd.from).await;
    }

    /// Adds `user` to `group`'s lobby. Answers go to the player privately,
    /// as in the legacy `AddPlayer` (Werewolf.cs:685-761); the group hears
    /// about joins in the timer's 30-second batches.
    async fn join(&self, group: i64, user: &User) {
        let ctl = match self.slots.locked().get(&group) {
            Some(Slot::Joining(ctl)) => Some(ctl.clone()),
            _ => None,
        };
        let lang = self.settings(group).await.language;
        let Some(ctl) = ctl else {
            self.say(user.id, &self.texts().key(&lang, "NoGame", &[])).await;
            return;
        };
        if let Some((other, name)) = self.group_of(user.id) {
            if other != group {
                self.say(user.id, &self.texts().key(&lang, "AlreadyInGame", &[&escape(&name)]))
                    .await;
                return;
            }
        }
        let _ = self
            .store
            .players()
            .upsert(user.id, &user.name, user.username.as_deref(), unix_now())
            .await;

        let (result, group_name, link) = {
            let mut c = ctl.locked();
            let result = c.lobby.join(Candidate {
                telegram_id: user.id,
                name: user.name.clone(),
            });
            if result.is_ok() {
                c.unannounced.push((user.id, user.name.trim().to_string()));
                if c.lobby.should_close_for_joining() {
                    c.timer.force_start(); // the lobby is full
                }
            }
            (result, c.group_name.clone(), c.settings.link.clone())
        };
        let text = match result {
            Ok(()) => {
                let group = match link.as_deref() {
                    Some(_) => linked_group_name(&group_name, link.as_deref()),
                    None => escape(&group_name),
                };
                self.texts().key(&lang, "YouJoined", &[&group])
            }
            Err(JoinError::AlreadyJoined) => self.texts().get(
                &lang,
                "",
                "You've already joined the game in {0}.",
                &[&escape(&group_name)],
            ),
            Err(JoinError::InvalidName) => self.texts().key(&lang, "ChangeNameToJoin", &[&escape(&user.name)]),
            Err(JoinError::NameTaken) => self.texts().key(
                &lang,
                "NameExists",
                &[
                    &escape(&user.name),
                    &escape(user.username.as_deref().unwrap_or(&user.name)),
                ],
            ),
            Err(JoinError::Full) => self.texts().key(&lang, "PlayerLimitReached", &[]),
            Err(JoinError::NotJoining) => self.texts().key(&lang, "NoGame", &[]),
        };
        self.say(user.id, &text).await;
    }

    async fn run_join_timer(self: Arc<Self>, group: i64, ctl: Arc<Mutex<LobbyCtl>>) {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        ticker.tick().await; // the first tick is immediate
        loop {
            ticker.tick().await;
            let (events, announce, lang, show_ids, killed) = {
                let mut c = ctl.locked();
                if c.lobby.state() != control::lobby::LobbyState::Joining {
                    (vec![], vec![], String::new(), false, true)
                } else {
                    let n = c.lobby.players().len();
                    let has_new = !c.unannounced.is_empty();
                    let events = c.timer.tick(n, has_new);
                    let announce = if events.contains(&TimerEvent::AnnounceJoined) {
                        std::mem::take(&mut c.unannounced)
                    } else {
                        vec![]
                    };
                    (
                        events,
                        announce,
                        c.settings.language.clone(),
                        c.settings.is_on(Toggle::ShowIDs),
                        false,
                    )
                }
            };
            if killed {
                return; // /killgame or shutdown already cleaned up
            }
            for event in &events {
                let (text, button) = match event {
                    TimerEvent::AnnounceJoined => {
                        // Werewolf.cs:458: with Show IDs, one player per line.
                        let names = if show_ids {
                            announce
                                .iter()
                                .map(|(id, n)| format!("{} (ID: <code>{id}</code>)\n", escape(n)))
                                .collect::<String>()
                        } else {
                            format!(
                                "{} ",
                                announce.iter().map(|(_, n)| escape(n)).collect::<Vec<_>>().join(", ")
                            )
                        };
                        (self.texts().key(&lang, "HaveJoined", &[&names]), false)
                    }
                    TimerEvent::Reminder { seconds_left: 60 } => {
                        (self.texts().key(&lang, "MinuteLeftToJoin", &[]), true)
                    }
                    TimerEvent::Reminder { seconds_left } => (
                        self.texts()
                            .key(&lang, "SecondsLeftToJoin", &[&bold(&seconds_left.to_string())]),
                        true,
                    ),
                    TimerEvent::Extended { by, seconds_left } => {
                        let key = if *by > 0 { "SecondsAdded" } else { "SecondsRemoved" };
                        (
                            self.texts().key(
                                &lang,
                                key,
                                &[&bold(&by.abs().to_string()), &bold(&mm_ss(*seconds_left))],
                            ),
                            true,
                        )
                    }
                    TimerEvent::Closed => continue,
                };
                let keyboard = button.then(|| self.join_button(group, &lang));
                if let Ok(m) = self.telegram.send(group, &text, keyboard).await {
                    if button {
                        ctl.locked().join_messages.push(m);
                    }
                }
            }
            if events.contains(&TimerEvent::Closed) {
                break;
            }
        }
        self.close_joining(group, ctl).await;
    }

    async fn close_joining(self: &Arc<Self>, group: i64, ctl: Arc<Mutex<LobbyCtl>>) {
        let (outcome, buttons, lobby, settings, group_name, requested_mode) = {
            let mut c = ctl.locked();
            let outcome = c.lobby.try_start();
            (
                outcome,
                std::mem::take(&mut c.join_messages),
                c.lobby.clone(),
                c.settings.clone(),
                c.group_name.clone(),
                c.mode,
            )
        };
        for m in buttons {
            let _ = self.telegram.clear_keyboard(m).await;
        }
        let lang = settings.language.clone();
        match outcome {
            StartOutcome::Aborted(AbortReason::NotEnoughPlayers) => {
                self.say(group, &self.texts().key(&lang, "NotEnoughPlayers", &[])).await;
                self.free_slot(group);
            }
            StartOutcome::Aborted(AbortReason::Killed) => self.free_slot(group),
            StartOutcome::Started => {
                self.say(group, &self.texts().key(&lang, "StartingGameWait", &[])).await;
                let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
                let (handle, clicks) = GameHandle::new(seq, group);
                self.slots.locked().insert(group, Slot::Running(handle.clone()));
                let core = self.clone();
                let setup = GameSetup {
                    telegram: self.telegram.clone(),
                    texts: self.texts(),
                    store: self.store.clone(),
                    settings,
                    group_id: group,
                    group_name,
                    lobby,
                    requested_mode,
                    seed: self
                        .config
                        .seed
                        .map(|s| s.wrapping_add(seq))
                        .unwrap_or_else(rand::random),
                    handle,
                    clicks,
                    on_finish: Box::new(move || core.free_slot(group)),
                };
                self.spawner.spawn(Box::new(move || {
                    Box::pin(async move {
                        // The slot is freed by `play` itself, even when it
                        // panics; this only reports it. Shared state is
                        // behind locks that ignore poisoning.
                        if let Err(panic) = AssertUnwindSafe(game::play(setup)).catch_unwind().await {
                            log::error!("group {group}: game panicked: {}", panic_message(&*panic));
                        }
                    })
                }));
            }
        }
    }

    fn free_slot(&self, group: i64) {
        self.slots.locked().remove(&group);
        self.slot_freed.notify_waiters();
    }

    fn lobby_of(&self, group: i64) -> Option<Arc<Mutex<LobbyCtl>>> {
        match self.slots.locked().get(&group) {
            Some(Slot::Joining(c)) => Some(c.clone()),
            _ => None,
        }
    }

    fn game_of(&self, group: i64) -> Option<Arc<GameHandle>> {
        match self.slots.locked().get(&group) {
            Some(Slot::Running(h)) => Some(h.clone()),
            _ => None,
        }
    }

    // ----- group commands -------------------------------------------------

    async fn flee(&self, cmd: &Command) {
        let group = cmd.chat.id;
        let settings = self.settings(group).await;
        let lang = &settings.language;
        let name = escape(&cmd.from.name);
        if let Some(ctl) = self.lobby_of(group) {
            let (result, remaining) = {
                let mut c = ctl.locked();
                let r = c.lobby.leave(cmd.from.id);
                c.unannounced.retain(|(id, _)| *id != cmd.from.id);
                (r, c.lobby.players().len())
            };
            match result {
                Ok(()) => {
                    self.say(group, &self.texts().key(lang, "Flee", &[&name])).await;
                    self.say(
                        group,
                        &self
                            .texts()
                            .key(lang, "CountPlayersRemain", &[&bold(&remaining.to_string())]),
                    )
                    .await;
                }
                Err(LeaveError::NotInGame) | Err(LeaveError::NotJoining) => {
                    self.say(group, &self.texts().key(lang, "NotPlaying", &[])).await;
                }
            }
        } else if let Some(game) = self.game_of(group) {
            if !game.is_playing(cmd.from.id) {
                self.say(group, &self.texts().key(lang, "NotPlaying", &[])).await;
            } else if !settings.flee_allowed_while_running() {
                self.say(group, &self.texts().key(lang, "FleeDisabled", &[])).await;
            } else if !game.is_alive(cmd.from.id) {
                self.say(group, &self.texts().key(lang, "DeadFlee", &[])).await;
            } else {
                game.flee(cmd.from.id);
                self.say(group, &self.texts().key(lang, "Flee", &[&name])).await;
            }
        } else {
            self.say(group, &self.texts().key(lang, "NoGame", &[])).await;
        }
    }

    async fn force_start(&self, cmd: &Command) {
        match self.lobby_of(cmd.chat.id) {
            Some(ctl) => ctl.locked().timer.force_start(),
            None => {
                let lang = self.settings(cmd.chat.id).await.language;
                self.say(cmd.chat.id, &self.texts().key(&lang, "NoGame", &[])).await;
            }
        }
    }

    /// `GameCommands.cs::Extend` (153-187) + `Werewolf.cs::ExtendTime`.
    async fn extend(&self, cmd: &Command) {
        let group = cmd.chat.id;
        let settings = self.settings(group).await;
        let lang = &settings.language;
        let Some(ctl) = self.lobby_of(group) else {
            self.say(group, &self.texts().key(lang, "NoGame", &[])).await;
            return;
        };
        let is_admin = self.is_global_admin(cmd.from.id)
            || self.telegram.is_group_admin(group, cmd.from.id).await.unwrap_or(false);
        let seconds = cmd
            .args
            .split_whitespace()
            .next()
            .and_then(|a| a.parse().ok())
            .unwrap_or(30);
        let result = {
            let mut c = ctl.locked();
            let r = c.lobby.extend_time(cmd.from.id, is_admin, seconds);
            if let Ok(delta) = r {
                c.timer.extend(delta as i64);
            }
            r
        };
        let error = match result {
            Ok(_) => return, // the timer announces the new time on its next tick
            Err(ExtendError::NotInGame) => self.texts().key(lang, "NotPlaying", &[]),
            Err(ExtendError::NegativeRequiresAdmin) | Err(ExtendError::ExtendDisabled) => {
                self.texts().key(lang, "GroupAdminOnly", &[])
            }
            Err(ExtendError::AlreadyExtended) => self.texts().key(lang, "CantExtend", &[]),
            Err(ExtendError::NotJoining) => self.texts().key(lang, "NoGame", &[]),
        };
        self.say(group, &error).await;
    }

    async fn players(&self, cmd: &Command) {
        let group = cmd.chat.id;
        let lang = self.settings(group).await.language;
        let text = if let Some(ctl) = self.lobby_of(group) {
            let names: Vec<String> = ctl.locked().lobby.players().iter().map(|p| escape(&p.name)).collect();
            format!(
                "{}: {}\n{}",
                bold(&self.texts().get(&lang, "", "Players", &[])),
                names.len(),
                names.join("\n")
            )
        } else if let Some(game) = self.game_of(group) {
            match game.players_reply(&self.texts(), &lang) {
                PlayersReply::Throttled => return,
                PlayersReply::LatestList(list) => {
                    let _ = self
                        .telegram
                        .send_reply(group, &self.texts().key(&lang, "LatestList", &[]), list.id)
                        .await;
                    return;
                }
                PlayersReply::List(text) => text,
            }
        } else {
            self.texts().key(&lang, "NoGame", &[])
        };
        self.say(group, &text).await;
    }

    async fn kill_game(&self, cmd: &Command) {
        let group = cmd.chat.id;
        let lang = self.settings(group).await.language;
        if let Some(ctl) = self.lobby_of(group) {
            let _ = ctl.locked().lobby.kill();
            let buttons = std::mem::take(&mut ctl.locked().join_messages);
            for m in buttons {
                let _ = self.telegram.clear_keyboard(m).await;
            }
            self.free_slot(group);
            self.say(group, &self.texts().get(&lang, "", "The game has been cancelled.", &[]))
                .await;
        } else if let Some(game) = self.game_of(group) {
            game.kill();
        } else {
            self.say(group, &self.texts().key(&lang, "NoGame", &[])).await;
        }
    }

    fn command_message(cmd: &Command) -> CommandMessage {
        CommandMessage {
            sender_id: cmd.from.id,
            reply_to: cmd.reply_to,
            mentions: cmd.mentions.clone(),
            argument: (!cmd.args.is_empty()).then(|| cmd.args.clone()),
        }
    }

    /// `/smite` (AdminCommands.cs + Werewolf.cs `FleePlayer`, 5360-5390): a
    /// forced flee with none of `/flee`'s checks.
    async fn smite(&self, cmd: &Command) {
        let group = cmd.chat.id;
        let targets = match self
            .moderation
            .lock()
            .await
            .smite_targets(&Self::command_message(cmd))
            .await
        {
            Ok(t) => t,
            Err(e) => {
                log::error!("smite: {e}");
                return;
            }
        };
        self.smite_ids(group, &targets).await;
    }

    async fn smite_ids(&self, group: i64, targets: &[i64]) {
        let lang = self.settings(group).await.language;
        for &target in targets {
            if let Some(ctl) = self.lobby_of(group) {
                let name = {
                    let mut c = ctl.locked();
                    let name = c
                        .lobby
                        .players()
                        .iter()
                        .find(|p| p.telegram_id == target)
                        .map(|p| p.name.clone());
                    if name.is_some() {
                        let _ = c.lobby.leave(target);
                    }
                    name
                };
                if let Some(name) = name {
                    self.say(group, &self.texts().key(&lang, "Flee", &[&escape(&name)]))
                        .await;
                }
            } else if let Some(game) = self.game_of(group) {
                if game.is_initializing() {
                    let text = "Cannot flee while game is initializing.  Try again once game is done starting.";
                    self.say(group, text).await;
                    return;
                }
                if game.is_alive(target) {
                    game.flee(target);
                    let name = self
                        .store
                        .players()
                        .get(target)
                        .await
                        .ok()
                        .flatten()
                        .map(|p| p.name)
                        .unwrap_or_default();
                    self.say(group, &self.texts().key(&lang, "Flee", &[&escape(&name)]))
                        .await;
                }
            }
        }
    }

    async fn moderation_command(&self, cmd: &Command, lang: &str) {
        let now = Timestamp(unix_now());
        let message = Self::command_message(cmd);
        let mut moderation = self.moderation.lock().await;
        let result: Result<(), db::DbError> = async {
            match cmd.name.as_str() {
                "getban" => {
                    if let Some(report) = moderation.getban(&message, now).await? {
                        self.say(cmd.chat.id, &report).await;
                    }
                }
                "permban" => {
                    let report = moderation.permban(&message, now).await?;
                    for reply in &report.replies {
                        self.say(cmd.chat.id, reply).await;
                    }
                    if cmd.chat.is_group() && !report.smite.is_empty() {
                        self.smite_ids(cmd.chat.id, &report.smite).await;
                    }
                    return Ok(());
                }
                "remban" => {
                    let report = moderation.remban(&message, now).await?;
                    for reply in &report.replies {
                        self.say(cmd.chat.id, reply).await;
                    }
                }
                "getidles" => {
                    let report = moderation
                        .getidles(&message, cmd.chat.id, now, self.texts().pack(lang))
                        .await?;
                    self.say(cmd.chat.id, &report).await;
                }
                "restore" => {
                    let check = moderation
                        .restore_check(Some(&cmd.args).filter(|a| !a.is_empty()).map(|s| s.as_str()), now)
                        .await?;
                    let keyboard = match &check {
                        RestoreCheck::Ready { old, new, .. } => Some(vec![vec![
                            Button::Callback {
                                text: self.texts().key(lang, "Yes", &[]),
                                data: format!("restore|{old}|{new}"),
                            },
                            Button::Callback {
                                text: self.texts().key(lang, "No", &[]),
                                data: "restore|no".into(),
                            },
                        ]]),
                        _ => None,
                    };
                    let _ = self.telegram.send(cmd.chat.id, check.reply(), keyboard).await;
                }
                _ => {}
            }
            Ok(())
        }
        .await;
        if let Err(e) = result {
            log::error!("/{}: {e}", cmd.name);
        }
    }
}

/// What a panic said, when it said it with a string (`panic!` and
/// `unwrap` always do).
fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("(no message)")
}

/// For tests and the dry-run harness: build a `Chat` for a group.
pub fn group_chat(id: i64, title: &str) -> Chat {
    Chat {
        id,
        kind: crate::telegram::ChatKind::Group { title: title.into() },
    }
}

pub fn private_chat(id: i64) -> Chat {
    Chat {
        id,
        kind: crate::telegram::ChatKind::Private,
    }
}
