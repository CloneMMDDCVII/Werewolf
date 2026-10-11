//! The storage scenarios from `telegram-bot-and-storage.feature`, run
//! against both backends. SQLite runs everywhere; Postgres runs when
//! `WEREWOLF_TEST_POSTGRES_URL` points at a server (CI starts one), each
//! test in a fresh database of its own.

use db::*;
use serde::{Deserialize, Serialize};
use shared::{GameMode, KillMethod, Role, Team};
use std::future::Future;
use std::sync::atomic::{AtomicU32, Ordering};

static N: AtomicU32 = AtomicU32::new(0);

async fn fresh_postgres() -> Option<Store> {
    let admin_url = std::env::var("WEREWOLF_TEST_POSTGRES_URL").ok()?;
    sqlx::any::install_default_drivers();
    let name = format!("wolf_t_{}_{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
    let admin = sqlx::AnyPool::connect(&admin_url)
        .await
        .expect("connect to WEREWOLF_TEST_POSTGRES_URL");
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE IF EXISTS {name}")))
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    let base = admin_url.rsplit_once('/').map(|(b, _)| b).unwrap_or(&admin_url);
    Some(
        Store::connect(&format!("{base}/{name}"))
            .await
            .expect("connect to fresh test database"),
    )
}

async fn both<F, Fut>(scenario: F)
where
    F: Fn(Store) -> Fut,
    Fut: Future<Output = ()>,
{
    scenario(Store::in_memory().await.unwrap()).await;
    match fresh_postgres().await {
        Some(store) => {
            assert_eq!(store.backend(), Backend::Postgres);
            scenario(store).await
        }
        None => eprintln!("WEREWOLF_TEST_POSTGRES_URL not set: Postgres half of this test not run"),
    }
}

// --- Health -----------------------------------------------------------

#[tokio::test]
async fn ping_answers_while_open_and_fails_once_closed() {
    both(|store| async move {
        store.ping().await.unwrap();
        store.close().await;
        assert!(store.ping().await.is_err());
    })
    .await;
}

// --- Group settings ---------------------------------------------------

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
struct SettingsV1 {
    night_timer: i64,
    language: String,
}

impl Default for SettingsV1 {
    fn default() -> Self {
        SettingsV1 {
            night_timer: 90,
            language: "English".into(),
        }
    }
}

/// The same settings after a field was added in a later release.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
struct SettingsV2 {
    night_timer: i64,
    language: String,
    allow_flee: bool,
}

impl Default for SettingsV2 {
    fn default() -> Self {
        SettingsV2 {
            night_timer: 90,
            language: "English".into(),
            allow_flee: true,
        }
    }
}

#[tokio::test]
async fn group_settings_survive_a_restart() {
    // A real restart: close the file-backed store and open it again.
    let dir = std::env::temp_dir().join(format!("wolf-restart-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let url = format!("sqlite://{}", dir.join("werewolf.db").display());

    let store = Store::connect(&url).await.unwrap();
    let settings = SettingsV1 {
        night_timer: 120,
        ..Default::default()
    };
    store
        .groups()
        .put_settings(-100, "Village Square", &settings, 1)
        .await
        .unwrap();
    store.close().await;

    let store = Store::connect(&url).await.unwrap();
    let back: SettingsV1 = store.groups().settings(-100).await.unwrap();
    assert_eq!(back.night_timer, 120);
}

#[tokio::test]
async fn group_settings_persist_on_both_backends() {
    both(|store| async move {
        let settings = SettingsV1 {
            night_timer: 120,
            ..Default::default()
        };
        store
            .groups()
            .put_settings(-100, "Village Square", &settings, 1)
            .await
            .unwrap();
        let back: SettingsV1 = store.groups().settings(-100).await.unwrap();
        assert_eq!(back, settings);
    })
    .await;
}

#[tokio::test]
async fn a_group_with_no_stored_settings_gets_defaults_and_nothing_is_written() {
    both(|store| async move {
        let s: SettingsV1 = store.groups().settings(-200).await.unwrap();
        assert_eq!(s, SettingsV1::default());
        assert_eq!(store.groups().get(-200).await.unwrap(), None);
    })
    .await;
}

#[tokio::test]
async fn new_settings_fields_need_no_migration() {
    both(|store| async move {
        let old = SettingsV1 {
            night_timer: 45,
            language: "Deutsch".into(),
        };
        store.groups().put_settings(-300, "Dorf", &old, 1).await.unwrap();
        let new: SettingsV2 = store.groups().settings(-300).await.unwrap();
        assert_eq!(
            new,
            SettingsV2 {
                night_timer: 45,
                language: "Deutsch".into(),
                allow_flee: true
            }
        );
    })
    .await;
}

#[tokio::test]
async fn renaming_a_group_keeps_its_settings() {
    both(|store| async move {
        let s = SettingsV1 {
            night_timer: 30,
            ..Default::default()
        };
        store
            .groups()
            .put_settings(-400, "Village Square", &s, 1)
            .await
            .unwrap();
        store.groups().upsert_name(-400, "Village Square II", 2).await.unwrap();
        let g = store.groups().get(-400).await.unwrap().unwrap();
        assert_eq!(g.name, "Village Square II");
        let back: SettingsV1 = store.groups().settings(-400).await.unwrap();
        assert_eq!(back.night_timer, 30);
    })
    .await;
}

// --- Players ----------------------------------------------------------

#[tokio::test]
async fn players_are_remembered_when_they_start_the_bot() {
    both(|store| async move {
        store.players().upsert(7, "Ana", Some("ana_w"), 1).await.unwrap();
        store.players().set_reachable(7, true, 1).await.unwrap();
        let p = store.players().get(7).await.unwrap().unwrap();
        assert_eq!(p.name, "Ana");
        assert_eq!(p.username.as_deref(), Some("ana_w"));
        assert!(p.reachable);
    })
    .await;
}

#[tokio::test]
async fn a_players_name_is_refreshed_and_reachability_kept() {
    both(|store| async move {
        store.players().upsert(7, "Ana", None, 1).await.unwrap();
        store.players().set_reachable(7, true, 1).await.unwrap();
        store.players().set_language(7, Some("Deutsch"), 1).await.unwrap();
        store.players().upsert(7, "Ana B", None, 2).await.unwrap();
        let p = store.players().get(7).await.unwrap().unwrap();
        assert_eq!(p.name, "Ana B");
        assert!(p.reachable);
        assert_eq!(p.language.as_deref(), Some("Deutsch"));
    })
    .await;
}

// --- Bans -------------------------------------------------------------

const DAY: i64 = 86_400;

fn ban(telegram_id: i64, expires_at: Option<i64>) -> NewBan {
    NewBan {
        telegram_id,
        reason: "spam".into(),
        issued_by: BanIssuer::Admin { telegram_id: 1 },
        issued_at: 0,
        expires_at,
        source: BanSource::Native,
    }
}

#[tokio::test]
async fn expired_bans_stop_counting_but_stay_in_history() {
    both(|store| async move {
        let now = 10 * DAY;
        store.bans().insert(&ban(7, Some(now - DAY))).await.unwrap(); // Ana: expired yesterday
        store.bans().insert(&ban(8, None)).await.unwrap(); // Bob: permanent

        assert!(!store.bans().is_banned(7, now).await.unwrap());
        assert!(store.bans().is_banned(8, now).await.unwrap());
        assert_eq!(store.bans().history(7).await.unwrap().len(), 1);

        let active: Vec<i64> = store
            .bans()
            .all_active(now)
            .await
            .unwrap()
            .iter()
            .map(|b| b.telegram_id)
            .collect();
        assert_eq!(active, vec![8]);
    })
    .await;
}

#[tokio::test]
async fn the_longest_ban_in_force_is_reported() {
    both(|store| async move {
        store.bans().insert(&ban(9, Some(5 * DAY))).await.unwrap();
        store.bans().insert(&ban(9, None)).await.unwrap();
        store.bans().insert(&ban(9, Some(7 * DAY))).await.unwrap();
        let b = store.bans().active(9, DAY).await.unwrap().unwrap();
        assert_eq!(b.expires_at, None);
    })
    .await;
}

#[tokio::test]
async fn lifting_a_ban_keeps_it_in_history() {
    both(|store| async move {
        store.bans().insert(&ban(8, None)).await.unwrap();
        assert_eq!(store.bans().lift(8, 1, DAY).await.unwrap(), 1);
        assert!(!store.bans().is_banned(8, DAY + 1).await.unwrap());
        let h = store.bans().history(8).await.unwrap();
        assert_eq!(h[0].lifted_at, Some(DAY));
        assert_eq!(h[0].lifted_by, Some(1));
    })
    .await;
}

#[tokio::test]
async fn ban_issuers_and_sources_round_trip() {
    both(|store| async move {
        let mut b = ban(5, None);
        b.issued_by = BanIssuer::System { rule: "idle".into() };
        b.source = BanSource::Migrated;
        store.bans().insert(&b).await.unwrap();
        let back = store.bans().active(5, 0).await.unwrap().unwrap();
        assert_eq!(back.issued_by, BanIssuer::System { rule: "idle".into() });
        assert_eq!(back.source, BanSource::Migrated);
    })
    .await;
}

// --- Games ------------------------------------------------------------

fn new_game() -> NewGame {
    NewGame {
        group_id: -100,
        group_name: "Village Square".into(),
        mode: GameMode::Normal,
        started_at: 1_782_888_320,
        players: vec![
            NewGamePlayer {
                telegram_id: 1,
                name: "Ana".into(),
                role: Role::Seer,
            },
            NewGamePlayer {
                telegram_id: 2,
                name: "Bob".into(),
                role: Role::Wolf,
            },
            NewGamePlayer {
                telegram_id: 3,
                name: "Cal".into(),
                role: Role::Traitor,
            },
            NewGamePlayer {
                telegram_id: 4,
                name: "Dee".into(),
                role: Role::Villager,
            },
            NewGamePlayer {
                telegram_id: 5,
                name: "Eve".into(),
                role: Role::Villager,
            },
        ],
    }
}

#[tokio::test]
async fn every_finished_game_is_recorded() {
    both(|store| async move {
        let games = store.games();
        let id = games.start(&new_game()).await.unwrap();
        assert_eq!(games.unfinished().await.unwrap().len(), 1);

        games
            .record_kill(id, 1, Some(2), KillMethod::Eat, 1, 1_782_888_400)
            .await
            .unwrap();
        games.set_final_role(id, 3, Role::Wolf).await.unwrap();
        games
            .record_kill(id, 2, None, KillMethod::Lynch, 1, 1_782_888_500)
            .await
            .unwrap();
        games
            .record_kill(id, 3, None, KillMethod::Lynch, 2, 1_782_888_600)
            .await
            .unwrap();
        games
            .finish(
                id,
                &GameResult::Won(Team::Village),
                1_782_888_700,
                &[
                    (1, false, true),
                    (2, false, false),
                    (3, false, false),
                    (4, true, true),
                    (5, true, true),
                ],
            )
            .await
            .unwrap();

        assert!(games.unfinished().await.unwrap().is_empty());
        assert_eq!(games.result(id).await.unwrap(), Some(GameResult::Won(Team::Village)));
        assert!(store.players().has_played(4).await.unwrap());
        assert!(!store.players().has_played(99).await.unwrap());

        let f = games.export_fixture(id).await.unwrap();
        assert_eq!(f.winner, "Village");
        assert_eq!(f.game_mode, "Normal");
        assert_eq!(f.time_started, "2026-07-01T06:45:20.000");
        let cal = f.players.iter().find(|p| p.telegram_id == 3).unwrap();
        assert_eq!(cal.role, "Wolf", "the export uses the role the player ended with");
        assert_eq!(f.kills.len(), 3);
        assert_eq!(f.kills[0].killer_name, "Bob");
        assert_eq!(f.kills[1].killer_telegram_id, 0);
        assert_eq!(f.kills[2].day, 2);
    })
    .await;
}

#[tokio::test]
async fn games_cut_off_by_a_crash_are_found_and_marked_interrupted() {
    both(|store| async move {
        let id = store.games().start(&new_game()).await.unwrap();
        let open = store.games().unfinished().await.unwrap();
        assert_eq!(open[0].id, id);
        assert_eq!(open[0].group_name, "Village Square");

        store.games().mark_interrupted(id, 5).await.unwrap();
        assert!(store.games().unfinished().await.unwrap().is_empty());
        assert_eq!(store.games().result(id).await.unwrap(), Some(GameResult::Interrupted));
        assert!(store.games().export_fixture(id).await.is_err());
    })
    .await;
}

#[tokio::test]
async fn recorded_games_export_as_replayable_fixtures() {
    let store = Store::in_memory().await.unwrap();
    let id = store.games().start(&new_game()).await.unwrap();
    store
        .games()
        .record_kill(id, 2, None, KillMethod::Lynch, 1, 1_782_888_500)
        .await
        .unwrap();
    store
        .games()
        .finish(id, &GameResult::Won(Team::Village), 1_782_888_700, &[(2, false, false)])
        .await
        .unwrap();
    let game = store.games().export_fixture(id).await.unwrap();

    let path = std::env::temp_dir().join(format!("wolf-export-{}.json", std::process::id()));
    std::fs::write(
        &path,
        serde_json::to_string(&serde_json::json!({ "games": [game] })).unwrap(),
    )
    .unwrap();
    let loaded = sim::fixture::load_fixtures(&path).expect("sim loads the exported game");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].winner, shared::Winner::Village);
    assert_eq!(loaded[0].kills[0].method, KillMethod::Lynch);
}
