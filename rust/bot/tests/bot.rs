//! Whole-bot tests: real core, real games, real storage (in-memory SQLite),
//! with `FakeTelegram` standing in for Telegram and tokio's paused clock
//! fast-forwarding every timer. No token, no network.

use bot::core::{group_chat, private_chat, BotCore, CoreConfig, MaxGames};
use bot::fake::FakeTelegram;
use bot::spawn::LocalSpawner;
use bot::telegram::{Button, Command, Incoming, Retrying, User};
use bot::text::Texts;
use db::{GameId, GameResult};
use db::Store;
use std::sync::Arc;
use std::time::Duration;

const GROUP: i64 = -100;
const OTHER_GROUP: i64 = -200;
/// Group admin of GROUP, and the player who starts games.
const ADMIN: i64 = 1;
/// The bot operator (`global_admins`).
const DEV: i64 = 999;

struct Bot {
    tg: Arc<FakeTelegram>,
    core: Arc<BotCore>,
    store: Store,
    texts: Texts,
}

fn user(id: i64) -> User {
    User { id, name: format!("Player{id}"), username: Some(format!("player{id}")) }
}

impl Bot {
    async fn new(max_games: usize) -> Bot {
        Self::with_store(max_games, Store::in_memory().await.unwrap()).await
    }

    async fn with_store(max_games: usize, store: Store) -> Bot {
        Self::build(max_games, store, 7).await
    }

    async fn with_seed(seed: u64) -> Bot {
        Self::build(10, Store::in_memory().await.unwrap(), seed).await
    }

    async fn build(max_games: usize, store: Store, seed: u64) -> Bot {
        let tg = Arc::new(FakeTelegram::default());
        tg.make_admin(GROUP, ADMIN);
        tg.make_admin(OTHER_GROUP, ADMIN);
        let core = BotCore::new(
            Arc::new(Retrying::new(tg.clone())),
            Arc::new(Texts::embedded()),
            store.clone(),
            Arc::new(LocalSpawner::new()),
            Box::new(MaxGames(max_games)),
            CoreConfig { default_language: "English".into(), global_admins: vec![DEV], seed: Some(seed) },
        )
        .await
        .unwrap();
        Bot { tg, core, store, texts: Texts::embedded() }
    }

    fn t(&self, key: &str, args: &[&str]) -> String {
        self.texts.key("English", key, args)
    }

    /// Whether `text` is some variant of `key` (keys with several
    /// variants are picked at random).
    fn is(&self, text: &str, key: &str, args: &[&str]) -> bool {
        let variants = self.texts.pack("English").variants(key).unwrap_or_else(|| panic!("no key {key}"));
        variants.iter().any(|v| text.contains(&bot::text::fill(v, args)))
    }

    fn said(&self, chat: i64, key: &str, args: &[&str]) -> bool {
        self.is(&self.tg.transcript(chat), key, args)
    }

    async fn cmd(&self, chat: i64, from: i64, text: &str) {
        let (name, args) = text.trim_start_matches('/').split_once(' ').unwrap_or((text.trim_start_matches('/'), ""));
        let chat = if chat < 0 { group_chat(chat, "Wolf Pack") } else { private_chat(chat) };
        self.core.handle(Incoming::Command(Command::new(chat, user(from), name, args))).await;
    }

    async fn press(&self, from: i64, chat: i64, label: &str) {
        let (msg, button) = self.tg.find_button(chat, label).unwrap_or_else(|| panic!("no {label:?} button in {chat}"));
        let Button::Callback { data, .. } = button else { panic!("{label:?} is a link") };
        self.core
            .handle(Incoming::Callback { id: format!("cb{}", msg.id), from: user(from), message: Some(msg), data })
            .await;
    }

    /// Players who have started the bot privately, so it can PM them.
    async fn register(&self, ids: impl IntoIterator<Item = i64>) {
        for id in ids {
            self.cmd(id, id, "/start").await;
        }
    }

    /// `/startgame` by ADMIN, everyone in `players` joins, ADMIN forces the
    /// start.
    async fn start_game(&self, players: std::ops::RangeInclusive<i64>) {
        self.register(players.clone()).await;
        self.cmd(GROUP, ADMIN, "/startgame").await;
        for id in players {
            if id != ADMIN {
                self.cmd(GROUP, id, "/join").await;
            }
        }
        self.cmd(GROUP, ADMIN, "/forcestart").await;
        tick(2).await;
        assert_eq!(self.core.running_games(), 1, "{}", self.tg.transcript(GROUP));
    }

    /// Every player answers every question with its first option, a second
    /// at a time, until no game is left (or `limit` seconds pass).
    async fn play_out(&self, players: std::ops::RangeInclusive<i64>, limit: u32) {
        let skip = self.t("Skip", &[]);
        for _ in 0..limit {
            if self.core.active_groups() == 0 {
                return;
            }
            for id in players.clone() {
                for m in self.tg.open_keyboards(id) {
                    let first = m.keyboard.iter().flatten().flatten().find_map(|b| match b {
                        Button::Callback { text, data } if *text != skip && data.starts_with("q:") => Some(data.clone()),
                        _ => None,
                    });
                    if let Some(data) = first {
                        self.core
                            .handle(Incoming::Callback { id: format!("cb{}", m.msg.id), from: user(id), message: Some(m.msg), data })
                            .await;
                    }
                }
            }
            tick(1).await;
        }
        panic!("game still running after {limit}s:\n{}", self.tg.transcript(GROUP));
    }
}

async fn tick(secs: u64) {
    tokio::time::sleep(Duration::from_secs(secs)).await;
}

/// See `local!`. Short enough that no game timer is skipped past, long
/// enough that hours of game time pass in seconds.
const HEARTBEAT: Duration = Duration::from_millis(20);

/// Runs a test body on a `LocalSet` with tokio's clock paused, the way
/// `LocalSpawner` needs.
///
/// SQLite runs on its own thread, which the paused clock can't see: while
/// the runtime waits on it, tokio thinks everything is asleep and jumps
/// the clock to the next timer (a game deadline, or the pool's acquire
/// timeout). A timer that is always `HEARTBEAT` away keeps those jumps
/// small.
macro_rules! local {
    ($body:expr) => {
        tokio::task::LocalSet::new()
            .run_until(async {
                let heartbeat = tokio::task::spawn_local(async {
                    loop {
                        tokio::time::sleep(HEARTBEAT).await;
                    }
                });
                let out = $body.await;
                heartbeat.abort();
                out
            })
            .await
    };
}

#[tokio::test(start_paused = true)]
async fn a_full_game_plays_to_the_end_and_is_recorded() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.start_game(1..=7).await;
        // Everyone got their role privately.
        for id in 1..=7 {
            assert!(!bot.tg.texts(id).is_empty(), "player {id} got no role");
        }
        bot.play_out(1..=7, 3 * 3600).await;

        let group = bot.tg.transcript(GROUP);
        assert!(bot.is(&group, "StartingGameWait", &[]));
        assert!(bot.is(&group, "PlayersAlive", &[]), "the end-of-game summary: {group}");
        assert!(bot.store.games().unfinished().await.unwrap().is_empty());
        match bot.store.games().result(GameId(1)).await.unwrap() {
            Some(GameResult::Won(_)) | Some(GameResult::Unresolved(_)) => {}
            other => panic!("unexpected result {other:?}\n{group}"),
        }
        // No button is left open anywhere once the game is over.
        for id in 1..=7 {
            assert!(bot.tg.open_keyboards(id).is_empty(), "player {id} still has buttons");
        }
    })
}

#[tokio::test(start_paused = true)]
async fn several_seeds_all_finish() {
    local!(async {
        for players in [5, 8, 12] {
            let bot = Bot::new(10).await;
            bot.start_game(1..=players).await;
            bot.play_out(1..=players, 4 * 3600).await;
            assert!(bot.store.games().unfinished().await.unwrap().is_empty());
        }
    })
}

#[tokio::test(start_paused = true)]
async fn nobody_answering_still_ends_the_game() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.start_game(1..=5).await;
        for _ in 0..24 * 3600 {
            if bot.core.active_groups() == 0 {
                break;
            }
            tick(1).await;
        }
        assert_eq!(bot.core.active_groups(), 0);
        assert!(bot.said(1, "TimesUp", &[]) || bot.tg.open_keyboards(1).is_empty());
    })
}

#[tokio::test(start_paused = true)]
async fn the_join_timer_reminds_then_gives_up_without_enough_players() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.register([ADMIN]).await;
        bot.cmd(GROUP, ADMIN, "/startgame").await;
        assert!(bot.said(ADMIN, "YouJoined", &["Wolf Pack"]));
        tick(185).await;
        let group = bot.tg.transcript(GROUP);
        assert!(group.contains("Player1"));
        assert!(bot.is(&group, "MinuteLeftToJoin", &[]));
        assert!(bot.is(&group, "SecondsLeftToJoin", &["<b>30</b>"]));
        assert!(bot.is(&group, "SecondsLeftToJoin", &["<b>10</b>"]));
        assert!(bot.is(&group, "NotEnoughPlayers", &[]), "{group}");
        assert_eq!(bot.core.active_groups(), 0);
        // The Join buttons were taken away when joining closed.
        assert!(bot.tg.open_keyboards(GROUP).is_empty());
    })
}

#[tokio::test(start_paused = true)]
async fn joins_are_announced_in_batches() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.register(1..=3).await;
        bot.cmd(GROUP, ADMIN, "/startgame").await;
        bot.cmd(GROUP, 2, "/join").await;
        bot.cmd(GROUP, 3, "/join").await;
        tick(31).await;
        let joined: Vec<String> =
            bot.tg.texts(GROUP).into_iter().filter(|t| t.contains("Player2") && t.contains("Player3")).collect();
        assert_eq!(joined.len(), 1, "{}", bot.tg.transcript(GROUP));
    })
}

#[tokio::test(start_paused = true)]
async fn players_who_never_started_the_bot_are_sent_to_it() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.register([ADMIN]).await;
        bot.cmd(GROUP, ADMIN, "/startgame").await;
        bot.cmd(GROUP, 5, "/join").await;
        let link = format!("@{}", { use bot::telegram::Telegram; bot.tg.bot_username().to_string() });
        assert!(bot.said(GROUP, "PMTheBot", &["Player5", &link]));
        // The button starts the bot with a join payload, which joins them.
        let (_, button) = bot.tg.find_button(GROUP, &bot.t("StartMe", &[])).unwrap();
        let Button::Url { url, .. } = button else { panic!() };
        let payload = url.split("?start=").nth(1).unwrap().to_string();
        bot.cmd(5, 5, &format!("/start {payload}")).await;
        assert!(bot.said(5, "YouJoined", &["Wolf Pack"]));
    })
}

#[tokio::test(start_paused = true)]
async fn extend_flee_and_players_in_the_lobby() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.register(1..=3).await;
        bot.cmd(GROUP, ADMIN, "/startgame").await;
        bot.cmd(GROUP, 2, "/join").await;
        bot.cmd(GROUP, 3, "/join").await;
        bot.cmd(GROUP, 2, "/extend 60").await;
        assert!(bot.tg.last(GROUP).unwrap().text.contains("group admin"), "players can't extend unless the group allows it");
        bot.cmd(GROUP, ADMIN, "/extend 60").await;
        tick(2).await;
        let group = bot.tg.transcript(GROUP);
        // About four minutes left; the exact second depends on where the
        // command lands between the timer's one-second ticks.
        assert!(group.contains("extended by <b>60</b>"), "{group}");
        assert!(group.contains("<b>04:00</b>") || group.contains("<b>03:5"), "{group}");
        bot.cmd(GROUP, 3, "/flee").await;
        assert!(bot.said(GROUP, "Flee", &["Player3"]));
        bot.cmd(GROUP, 4, "/flee").await;
        assert!(bot.said(GROUP, "NotPlaying", &[]));
        bot.cmd(GROUP, 2, "/players").await;
        let list = bot.tg.last(GROUP).unwrap().text;
        assert!(list.contains("Player1") && list.contains("Player2") && !list.contains("Player3"), "{list}");
    })
}

#[tokio::test(start_paused = true)]
async fn forcestart_is_for_group_admins() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.register(1..=5).await;
        bot.cmd(GROUP, ADMIN, "/startgame").await;
        for id in 2..=5 {
            bot.cmd(GROUP, id, "/join").await;
        }
        bot.cmd(GROUP, 2, "/forcestart").await;
        tick(2).await;
        assert_eq!(bot.core.running_games(), 0);
        bot.cmd(GROUP, ADMIN, "/forcestart").await;
        tick(2).await;
        assert_eq!(bot.core.running_games(), 1);
    })
}

#[tokio::test(start_paused = true)]
async fn one_game_per_group_and_none_in_private() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.register([ADMIN]).await;
        bot.cmd(ADMIN, ADMIN, "/startgame").await;
        assert_eq!(bot.core.active_groups(), 0);
        bot.cmd(GROUP, ADMIN, "/startgame").await;
        bot.cmd(GROUP, 2, "/startgame").await;
        assert!(bot.tg.last(GROUP).unwrap().text.contains("already being joined"));
        assert_eq!(bot.core.active_groups(), 1);
    })
}

#[tokio::test(start_paused = true)]
async fn a_player_can_only_be_in_one_lobby() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.register(1..=2).await;
        bot.cmd(GROUP, ADMIN, "/startgame").await;
        bot.cmd(OTHER_GROUP, 2, "/startgame").await;
        bot.cmd(OTHER_GROUP, ADMIN, "/join").await;
        assert!(bot.is(&bot.tg.last(ADMIN).unwrap().text, "AlreadyInGame", &["Wolf Pack"]));
    })
}

#[tokio::test(start_paused = true)]
async fn busy_bots_refuse_new_games() {
    local!(async {
        let bot = Bot::new(1).await;
        bot.start_game(1..=5).await;
        bot.cmd(OTHER_GROUP, 6, "/startgame").await;
        assert!(bot.tg.last(OTHER_GROUP).unwrap().text.contains("very busy"));
    })
}

#[tokio::test(start_paused = true)]
async fn killgame_ends_a_running_game() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.start_game(1..=5).await;
        bot.cmd(GROUP, ADMIN, "/killgame").await; // not a global admin: ignored
        tick(1).await;
        assert_eq!(bot.core.running_games(), 1);
        bot.cmd(GROUP, DEV, "/killgame").await;
        for _ in 0..120 {
            tick(1).await;
        }
        assert_eq!(bot.core.active_groups(), 0, "{}", bot.tg.transcript(GROUP));
        assert!(bot.tg.transcript(GROUP).contains("ended by an admin"));
        assert_eq!(bot.store.games().result(GameId(1)).await.unwrap(), Some(GameResult::Killed));
    })
}

#[tokio::test(start_paused = true)]
async fn buttons_from_a_finished_game_say_so() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.start_game(1..=5).await;
        // Keep one question around, then end the game.
        let stale = loop {
            if let Some(m) = (1..=5).flat_map(|id| bot.tg.open_keyboards(id)).next() {
                break m;
            }
            tick(1).await;
        };
        bot.cmd(GROUP, DEV, "/killgame").await;
        for _ in 0..120 {
            tick(1).await;
        }
        assert_eq!(bot.core.active_groups(), 0);
        let Some(Button::Callback { data, .. }) = stale.keyboard.as_ref().and_then(|k| k[0].first().cloned()) else {
            panic!()
        };
        bot.core
            .handle(Incoming::Callback { id: "late".into(), from: user(stale.msg.chat), message: Some(stale.msg), data })
            .await;
        let answers = bot.tg.callback_answers();
        assert_eq!(answers.last().unwrap(), &("late".to_string(), Some("That game is over.".to_string())));
    })
}

#[tokio::test(start_paused = true)]
async fn a_player_who_blocks_the_bot_mid_game_does_not_stop_it() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.start_game(1..=6).await;
        bot.tg.block(3);
        bot.play_out(1..=6, 4 * 3600).await;
        assert!(bot.store.games().unfinished().await.unwrap().is_empty());
    })
}

#[tokio::test(start_paused = true)]
async fn smite_removes_a_player_from_the_lobby() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.register(1..=3).await;
        bot.cmd(GROUP, ADMIN, "/startgame").await;
        bot.cmd(GROUP, 2, "/join").await;
        let mut smite = Command::new(group_chat(GROUP, "Wolf Pack"), user(ADMIN), "smite", "");
        smite.reply_to = Some(moderation::ReplyTo { from: 2, forwarded_from: None });
        bot.core.handle(Incoming::Command(smite)).await;
        assert!(bot.said(GROUP, "Flee", &["Player2"]), "{}", bot.tg.transcript(GROUP));
        bot.cmd(GROUP, ADMIN, "/players").await;
        assert!(!bot.tg.last(GROUP).unwrap().text.contains("Player2"));
    })
}

#[tokio::test(start_paused = true)]
async fn flood_control_is_waited_out() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.tg.flood_once(ADMIN, 3);
        let before = tokio::time::Instant::now();
        bot.cmd(ADMIN, ADMIN, "/start").await;
        assert!(before.elapsed() >= Duration::from_secs(3));
        assert!(bot.tg.transcript(ADMIN).contains("Werewolf moderator"));
        bot.cmd(ADMIN, ADMIN, "/help").await;
        assert!(bot.tg.last(ADMIN).unwrap().text.contains("Support Group"));
        bot.cmd(ADMIN, ADMIN, "/ping").await;
        assert!(bot.tg.last(ADMIN).unwrap().text.contains("s"), "ping edits its reply with the time");
    })
}

#[tokio::test(start_paused = true)]
async fn games_cut_off_by_a_crash_are_announced_on_restart() {
    local!(async {
        let store = Store::in_memory().await.unwrap();
        {
            let bot = Bot::with_store(10, store.clone()).await;
            bot.start_game(1..=5).await;
            // The process dies here: the game never records its end.
        }
        let bot = Bot::with_store(10, store.clone()).await;
        bot.core.announce_interrupted_games().await;
        assert!(bot.tg.transcript(GROUP).contains("bot restarted"));
        assert_eq!(store.games().result(GameId(1)).await.unwrap(), Some(GameResult::Interrupted));
    })
}

#[tokio::test(start_paused = true)]
async fn shutdown_lets_games_finish_or_stops_them() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.start_game(1..=5).await;
        bot.core.stop_accepting();
        bot.cmd(OTHER_GROUP, ADMIN, "/startgame").await;
        assert!(bot.tg.last(OTHER_GROUP).unwrap().text.contains("restarting"));
        bot.core.kill_everything().await;
        assert!(bot.tg.transcript(GROUP).contains("shutting down"));
        tokio::time::timeout(Duration::from_secs(120), bot.core.wait_until_idle()).await.expect("games stopped");
        assert_eq!(bot.store.games().result(GameId(1)).await.unwrap(), Some(GameResult::Killed));
    })
}

#[tokio::test(start_paused = true)]
async fn config_menu_goes_to_the_admin_privately() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.register([ADMIN]).await;
        bot.cmd(GROUP, 2, "/config").await;
        assert!(bot.tg.open_keyboards(2).is_empty());
        bot.cmd(GROUP, ADMIN, "/config").await;
        assert!(!bot.tg.open_keyboards(ADMIN).is_empty(), "{:?}", bot.tg.messages());
    })
}

#[tokio::test(start_paused = true)]
async fn setlang_and_getlang_in_private() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.register([ADMIN]).await;
        bot.cmd(ADMIN, ADMIN, "/setlang").await;
        assert!(!bot.tg.open_keyboards(ADMIN).is_empty());
        bot.cmd(ADMIN, ADMIN, "/getlang English").await;
        assert!(bot.tg.last(ADMIN).unwrap().text.starts_with("[file English.xml,"));
        bot.cmd(ADMIN, ADMIN, "/getlang ../secrets").await;
        assert!(bot.tg.last(ADMIN).unwrap().text.contains("Invalid language file name"));
        bot.cmd(ADMIN, ADMIN, "/getlang").await;
        bot.press(ADMIN, ADMIN, "English").await;
        assert!(bot.tg.last(ADMIN).unwrap().text.starts_with("[file English.xml,"));
    })
}

#[tokio::test(start_paused = true)]
async fn lynch_votes_are_announced_as_they_are_cast() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.start_game(1..=5).await;
        let ask_lynch = bot.texts.pack("English").variants("AskLynch").unwrap().to_vec();
        // Wait for the first lynch question to reach player 2.
        let question = loop {
            if let Some(m) = bot.tg.open_keyboards(2).into_iter().find(|m| ask_lynch.contains(&m.text)) {
                break m;
            }
            assert!(bot.core.active_groups() > 0, "game ended before a lynch vote");
            tick(1).await;
        };
        let Some(Button::Callback { text: target, data }) = question.keyboard.as_ref().and_then(|k| k[0].first().cloned()) else {
            panic!()
        };
        bot.core
            .handle(Incoming::Callback { id: "vote".into(), from: user(2), message: Some(question.msg), data })
            .await;
        tokio::time::sleep(Duration::from_millis(100)).await; // the game task picks the press up
        // Said in the group at once, while everyone else is still voting.
        assert!(bot.said(GROUP, "PlayerVotedLynch", &["Player2", &target]), "{}", bot.tg.transcript(GROUP));
        assert!(bot.tg.last(2).is_some());
        assert!(!bot.tg.open_keyboards(3).is_empty(), "the others are still voting");
    })
}

/// The newest open question in `chat` whose text is a variant of `key`.
fn question(bot: &Bot, chat: i64, key: &str) -> Option<bot::fake::FakeMessage> {
    let variants = bot.texts.pack("English").variants(key).unwrap().to_vec();
    bot.tg.open_keyboards(chat).into_iter().rev().find(|m| variants.contains(&m.text))
}

async fn click(bot: &Bot, from: i64, m: &bot::fake::FakeMessage, label: &str) {
    let data = m
        .keyboard
        .iter()
        .flatten()
        .flatten()
        .find_map(|b| match b {
            Button::Callback { text, data } if text == label => Some(data.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no {label:?} button"));
    bot.core.handle(Incoming::Callback { id: format!("cb{}", m.msg.id), from: user(from), message: Some(m.msg), data }).await;
    tokio::time::sleep(Duration::from_millis(100)).await; // the game task picks the press up
}

#[tokio::test(start_paused = true)]
async fn the_pacifists_peace_closes_the_lynch_vote() {
    local!(async {
        let players = 1..=8;
        // Deal until a Pacifist is still alive when the lynch vote opens.
        for seed in 0..60 {
            let bot = Bot::with_seed(seed).await;
            bot.start_game(players.clone()).await;
            let (pacifist, peace) = loop {
                if bot.core.active_groups() == 0 || players.clone().any(|id| question(&bot, id, "AskLynch").is_some()) {
                    break (0, None);
                }
                if let Some((id, m)) = players.clone().find_map(|id| question(&bot, id, "AskPacifist").map(|m| (id, m))) {
                    break (id, Some(m));
                }
                tick(1).await;
            };
            let Some(peace) = peace else { continue };
            // Let the discussion run out and the vote open.
            let ballot = loop {
                let open = players.clone().filter(|&id| id != pacifist).find_map(|id| question(&bot, id, "AskLynch").map(|m| (id, m)));
                if open.is_some() {
                    break open;
                }
                if bot.core.active_groups() == 0 {
                    break None;
                }
                tick(1).await;
            };
            let Some((voter, ballot)) = ballot else { continue };
            let target = ballot.keyboard.as_ref().unwrap()[0][0].text().to_string();
            click(&bot, voter, &ballot, &target).await;
            click(&bot, pacifist, &peace, &bot.t("Yes", &[])).await;

            let pacifist_name = format!("Player{pacifist}");
            assert!(bot.said(GROUP, "PacifistNoLynch", &[&pacifist_name]), "{}", bot.tg.transcript(GROUP));
            assert!(bot.said(GROUP, "PacifistNoLynchNow", &[]), "{}", bot.tg.transcript(GROUP));
            let still_voting: Vec<i64> =
                players.clone().filter(|&id| question(&bot, id, "AskLynch").is_some()).collect();
            assert!(still_voting.is_empty(), "lynch menus left open for {still_voting:?}");
            let closed = players.clone().filter(|&id| bot.tg.texts(id).iter().any(|t| bot.is(t, "LynchPeaceTimeout", &[])));
            assert!(closed.count() > 0, "open lynch menus should say the peace closed them");
            return;
        }
        panic!("no seed dealt a Pacifist who lived to the lynch vote");
    })
}

#[tokio::test(start_paused = true)]
async fn everyone_is_told_their_role_when_the_game_starts() {
    local!(async {
        let bot = Bot::new(10).await;
        bot.start_game(1..=6).await;
        let pack = bot.texts.pack("English");
        let infos: Vec<String> = shared::ALL_ROLES
            .iter()
            .filter_map(|r| pack.variants(&format!("RoleInfo{r:?}")))
            .flat_map(|v| v.iter().map(|t| bot::text::fill(t, &[])))
            .collect();
        let knows = |id: i64| bot.tg.texts(id).iter().any(|t| infos.iter().any(|i| t.starts_with(i.as_str())));
        // Storage runs on its own thread, so give the game task a few
        // (paused-clock) seconds to deal.
        for _ in 0..30 {
            if (1..=6).all(knows) {
                return;
            }
            tick(1).await;
        }
        let missing: Vec<i64> = (1..=6).filter(|&id| !knows(id)).collect();
        panic!("not told their role: {missing:?}");
    })
}
