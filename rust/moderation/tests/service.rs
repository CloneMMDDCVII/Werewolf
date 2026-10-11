//! The moderation commands end to end on an in-memory database.
use db::{NewGame, NewGamePlayer, Store};
use i18n::LanguagePack;
use moderation::{CommandMessage, Mention, Moderation, ReplyTo, RestoreCheck, Timestamp};
use shared::{GameMode, KillMethod, Role};

const LANGUAGE_PACK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../Werewolf for Telegram/Languages/English.xml"
);
const ALICE: i64 = 1;
const PAUL: i64 = 2;
const RITA: i64 = 3;
const PACK: i64 = -100;

fn now() -> Timestamp {
    Timestamp::from_utc(2026, 10, 10, 12, 0, 0)
}

async fn store() -> Store {
    let store = Store::in_memory().await.unwrap();
    let players = store.players();
    players.upsert(ALICE, "Alice", Some("alice"), 0).await.unwrap();
    players.upsert(PAUL, "Paul", Some("paul"), 0).await.unwrap();
    players.upsert(RITA, "Rita", None, 0).await.unwrap();
    store
}

fn from_alice(argument: &str) -> CommandMessage {
    CommandMessage {
        sender_id: ALICE,
        argument: Some(argument.into()),
        ..Default::default()
    }
}

async fn game(store: &Store, group: i64, started_at: i64, who: &[i64]) -> db::GameId {
    let players = who
        .iter()
        .map(|id| NewGamePlayer {
            telegram_id: *id,
            name: format!("P{id}"),
            role: Role::Villager,
        })
        .collect();
    let game = NewGame {
        group_id: group,
        group_name: "Pack".into(),
        mode: GameMode::Normal,
        started_at,
        players,
    };
    store.games().start(&game).await.unwrap()
}

#[tokio::test]
async fn a_ban_survives_a_restart_and_silences_the_player() {
    let store = store().await;
    let mut moderation = Moderation::load(store.clone(), now()).await.unwrap();
    let report = moderation.permban(&from_alice("2 cheating"), now()).await.unwrap();
    assert_eq!(report.replies, vec!["User Paul (@paul) has been banned"]);
    assert_eq!(report.smite, vec![PAUL]);

    let reloaded = Moderation::load(store.clone(), now()).await.unwrap();
    assert!(reloaded.is_banned(PAUL, now()));
    let saved = store.bans().active(PAUL, now().0).await.unwrap().unwrap();
    assert_eq!(saved.reason, "cheating");
    assert_eq!(saved.issued_by, db::BanIssuer::Admin { telegram_id: ALICE });
    assert_eq!(saved.expires_at, None);
}

#[tokio::test]
async fn permban_of_a_stranger_creates_their_player_record() {
    let store = store().await;
    let mut moderation = Moderation::load(store.clone(), now()).await.unwrap();
    moderation.permban(&from_alice("555 spam"), now()).await.unwrap();
    assert_eq!(store.players().get(555).await.unwrap().unwrap().name, "Unknown User");
}

#[tokio::test]
async fn getban_reads_the_stored_ban_and_first_game() {
    let store = store().await;
    game(&store, PACK, Timestamp::from_utc(2017, 3, 4, 18, 30, 0).0, &[PAUL]).await;
    let mut moderation = Moderation::load(store.clone(), now()).await.unwrap();
    moderation
        .permban(&from_alice("2 spam"), Timestamp::from_utc(2026, 10, 1, 12, 0, 0))
        .await
        .unwrap();

    let message = CommandMessage {
        mentions: vec![Mention::Username("Paul".into())],
        ..from_alice("@Paul")
    };
    let report = moderation.getban(&message, now()).await.unwrap().unwrap();
    assert_eq!(
        report,
        "Player: Paul\nCurrent Status: <b>Banned for: spam</b>\nBy: Alice on 01OCT2026 12:00:00 +00:00\n\
         <b>Perm Ban</b>\nPlayer first seen: 04MAR2017 18:30:00 +00:00"
    );
}

#[tokio::test]
async fn getban_with_no_target_reports_on_the_sender() {
    let moderation = Moderation::load(store().await, now()).await.unwrap();
    let report = moderation.getban(&from_alice(""), now()).await.unwrap().unwrap();
    assert!(report.starts_with("Player: Alice\nCurrent Status: Not banned (in Werewolf)"));
}

#[tokio::test]
async fn remban_lifts_but_keeps_history() {
    let store = store().await;
    let mut moderation = Moderation::load(store.clone(), now()).await.unwrap();
    moderation.permban(&from_alice("2 spam"), now()).await.unwrap();
    let message = CommandMessage {
        mentions: vec![Mention::Username("paul".into())],
        ..from_alice("@paul")
    };
    let report = moderation.remban(&message, now()).await.unwrap();
    assert_eq!(report.replies, vec!["User has been unbanned."]);
    assert!(!moderation.is_banned(PAUL, now()));
    assert!(!Moderation::load(store.clone(), now())
        .await
        .unwrap()
        .is_banned(PAUL, now()));
    let history = store.bans().history(PAUL).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].lifted_by, Some(ALICE));
}

#[tokio::test]
async fn an_expired_stored_ban_is_not_loaded() {
    let store = store().await;
    store
        .bans()
        .insert(&db::NewBan {
            telegram_id: PAUL,
            reason: "afk".into(),
            issued_by: db::BanIssuer::System { rule: "Para".into() },
            issued_at: 0,
            expires_at: Some(now().0 - 1),
            source: db::BanSource::Migrated,
        })
        .await
        .unwrap();
    assert!(!Moderation::load(store, now()).await.unwrap().is_banned(PAUL, now()));
}

#[tokio::test]
async fn smite_resolves_usernames_through_storage() {
    let moderation = Moderation::load(store().await, now()).await.unwrap();
    let message = CommandMessage {
        reply_to: Some(ReplyTo {
            from: RITA,
            forwarded_from: None,
        }),
        mentions: vec![Mention::Username("PAUL".into()), Mention::Username("ghost".into())],
        ..from_alice("@PAUL @ghost")
    };
    assert_eq!(moderation.smite_targets(&message).await.unwrap(), vec![RITA, PAUL]);
}

#[tokio::test]
async fn getidles_counts_the_last_24_hours() {
    let store = store().await;
    let here = game(&store, PACK, 0, &[PAUL]).await;
    let elsewhere = game(&store, -200, 0, &[PAUL]).await;
    let games = store.games();
    let hour = 3600;
    games
        .record_kill(here, PAUL, None, KillMethod::Idle, 1, now().0 - hour)
        .await
        .unwrap();
    games
        .record_kill(elsewhere, PAUL, None, KillMethod::Idle, 1, now().0 - 2 * hour)
        .await
        .unwrap();
    games
        .record_kill(elsewhere, PAUL, None, KillMethod::Idle, 2, now().0 - 3 * hour)
        .await
        .unwrap();
    games
        .record_kill(here, PAUL, None, KillMethod::Idle, 2, now().0 - 25 * hour)
        .await
        .unwrap();
    games
        .record_kill(here, PAUL, Some(RITA), KillMethod::Eat, 3, now().0 - hour)
        .await
        .unwrap();

    let pack = LanguagePack::load(LANGUAGE_PACK_PATH).unwrap();
    let moderation = Moderation::load(store, now()).await.unwrap();
    let report = moderation
        .getidles(&from_alice("2 3"), PACK, now(), &pack)
        .await
        .unwrap();
    assert_eq!(
        report,
        "Player 2 (Paul) has been killed for not voting 3 times in the past 24 hours. 1 of those kills were in this group.\n\
         Player 3 (Rita) has been killed for not voting 0 times in the past 24 hours. 0 of those kills were in this group.\n"
    );
}

#[tokio::test]
async fn restore_checks_then_moves_the_history() {
    let store = store().await;
    store.players().upsert(20, "Paul", Some("paul2"), 0).await.unwrap();
    let old_game = game(&store, PACK, 0, &[PAUL, RITA]).await;
    game(&store, PACK, 10, &[20]).await;
    store
        .games()
        .record_kill(old_game, PAUL, Some(RITA), KillMethod::Lynch, 1, 5)
        .await
        .unwrap();
    let moderation = Moderation::load(store.clone(), now()).await.unwrap();

    assert_eq!(
        moderation.restore_check(Some("2"), now()).await.unwrap(),
        RestoreCheck::Usage
    );
    assert_eq!(
        moderation.restore_check(Some("2 99"), now()).await.unwrap(),
        RestoreCheck::NotFound
    );
    let RestoreCheck::Ready { old, new, score, .. } = moderation.restore_check(Some("2 20"), now()).await.unwrap()
    else {
        panic!("expected a report");
    };
    // Only the username differs, by one character: 100 - (1 + 0) / 2.
    assert_eq!((old, new, score), (PAUL, 20, 100));

    assert_eq!(
        moderation.restore(old, new).await.unwrap(),
        "Restored stats from Paul to Paul"
    );
    let games = store.games().games_of(20).await.unwrap();
    assert_eq!(games.len(), 2);
    assert!(store.games().games_of(PAUL).await.unwrap().is_empty());
}

#[tokio::test]
async fn restore_refuses_a_banned_old_account() {
    let store = store().await;
    let mut moderation = Moderation::load(store, now()).await.unwrap();
    moderation.permban(&from_alice("2 spam"), now()).await.unwrap();
    assert_eq!(
        moderation.restore_check(Some("2 3"), now()).await.unwrap(),
        RestoreCheck::OldAccountBanned
    );
}
