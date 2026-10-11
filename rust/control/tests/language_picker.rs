//! `features/language_picker.feature`, one test per scenario, against the
//! real language files and an in-memory `db::Store`.

use control::group_config::{load_settings, main_menu, run_config_command, CommandChat, GroupSettings};
use control::language_picker::*;
use control::menu::*;
use db::Store;
use i18n::{Catalog, LanguageId};
use std::sync::OnceLock;

const GROUP: i64 = -1001234567890;
const PLAYER: i64 = 4242;

fn choices() -> &'static LanguageChoices {
    static CHOICES: OnceLock<LanguageChoices> = OnceLock::new();
    CHOICES.get_or_init(|| {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Werewolf for Telegram/Languages");
        LanguageChoices::from_catalog(&Catalog::load_dir(dir).expect("language directory should load"))
    })
}

fn admin() -> Actor {
    Actor { can_configure: true }
}

fn member() -> Actor {
    Actor { can_configure: false }
}

fn user() -> CommandUser {
    CommandUser {
        telegram_id: PLAYER,
        name: "Alice".into(),
        username: Some("alice".into()),
    }
}

fn raw_labels(screen: &Screen) -> Vec<String> {
    screen
        .rows
        .iter()
        .flatten()
        .map(|b| match &b.label {
            Text::Raw(s) | Text::Locale(s) => s.clone(),
        })
        .collect()
}

async fn setlang(store: &Store, in_private_chat: bool) -> (Screen, Option<Message>, Screen) {
    match run_setlang_command(store, choices(), &user(), in_private_chat, 1)
        .await
        .unwrap()
    {
        CommandOutcome::SendPrivately {
            menu,
            chat_notice,
            fallback,
        } => (menu, chat_notice, fallback),
        other => panic!("{other:?}"),
    }
}

async fn pick(store: &Store, target: LanguageTarget, step: LanguageStep, actor: Actor) -> CallbackOutcome {
    run_language_callback(store, choices(), &LanguageAction { target, step }, actor, 2)
        .await
        .unwrap()
}

fn shown(outcome: &CallbackOutcome) -> &Screen {
    match outcome {
        CallbackOutcome::Show { screen, .. } => screen,
        other => panic!("expected a screen, got {other:?}"),
    }
}

#[tokio::test]
async fn the_base_list_shows_every_base_language_once_sorted_two_per_row() {
    let store = Store::in_memory().await.unwrap();
    let (menu, _, _) = setlang(&store, true).await;
    assert_eq!(
        menu.message,
        Message::with("WhatLang", vec![Text::Raw("English".into())])
    );
    let bases = raw_labels(&menu);
    let mut sorted = bases.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(bases, sorted);
    assert!(bases.len() > 20, "expected the shipped bases, got {bases:?}");
    assert!(bases.contains(&"English".to_string()) && bases.contains(&"Thai".to_string()));
    assert!(menu.rows.iter().all(|r| r.len() <= 2));
}

#[tokio::test]
async fn setlang_in_a_group_answers_privately_and_says_so_in_the_group() {
    let store = Store::in_memory().await.unwrap();
    let (_, notice, _) = setlang(&store, false).await;
    assert_eq!(notice, Some(Message::new("SentPrivate")));
    let (_, notice, _) = setlang(&store, true).await;
    assert_eq!(notice, None);
}

#[tokio::test]
async fn a_player_who_never_started_the_bot_is_asked_to() {
    let store = Store::in_memory().await.unwrap();
    let (_, _, fallback) = setlang(&store, false).await;
    assert_eq!(fallback.message.key, "StartMe");
    assert_eq!(fallback.rows[0][0].action, ButtonAction::StartBotLink);
}

#[tokio::test]
async fn a_player_with_no_language_set_is_treated_as_english() {
    let store = Store::in_memory().await.unwrap();
    store.players().upsert(PLAYER, "Alice", None, 0).await.unwrap();
    assert_eq!(player_language(&store, PLAYER).await.unwrap(), "English");
    let (menu, _, _) = setlang(&store, true).await;
    assert_eq!(menu.message.args, vec![Text::Raw("English".into())]);
}

#[tokio::test]
async fn picking_a_base_with_one_file_sets_it_straight_away() {
    let store = Store::in_memory().await.unwrap();
    setlang(&store, true).await;
    assert_eq!(choices().variants("Thai").len(), 1);
    let outcome = pick(
        &store,
        LanguageTarget::Player(PLAYER),
        LanguageStep::PickBase(LanguageId::of("Thai")),
        member(),
    )
    .await;
    let thai = choices().variants("Thai")[0].file.clone();
    assert_eq!(player_language(&store, PLAYER).await.unwrap(), thai);
    assert_eq!(
        shown(&outcome).message,
        Message::with("LangSet", vec![Text::Raw("Thai: Normal".into())])
    );
    assert!(shown(&outcome).rows.is_empty());
}

#[tokio::test]
async fn picking_a_base_with_several_files_asks_for_the_variant() {
    let store = Store::in_memory().await.unwrap();
    setlang(&store, true).await;
    let outcome = pick(
        &store,
        LanguageTarget::Player(PLAYER),
        LanguageStep::PickBase(LanguageId::of("English")),
        member(),
    )
    .await;
    let screen = shown(&outcome);
    assert_eq!(
        screen.message,
        Message::with("WhatVariant", vec![Text::Raw("Normal".into())])
    );
    let labels = raw_labels(screen);
    assert_eq!(labels[0], "Normal", "the default variant comes first");
    let mut rest = labels[1..].to_vec();
    rest.sort();
    assert_eq!(labels[1..], rest[..]);
    assert!(labels.contains(&"Star Wars".to_string()));
    assert!(screen.rows.iter().all(|r| r.len() <= 2));
    assert_eq!(
        player_language(&store, PLAYER).await.unwrap(),
        "English",
        "nothing chosen yet"
    );
}

#[tokio::test]
async fn picking_a_variant_sets_the_language() {
    let store = Store::in_memory().await.unwrap();
    setlang(&store, true).await;
    let outcome = pick(
        &store,
        LanguageTarget::Player(PLAYER),
        LanguageStep::PickFile(LanguageId::of("English Star Wars")),
        member(),
    )
    .await;
    assert_eq!(player_language(&store, PLAYER).await.unwrap(), "English Star Wars");
    assert_eq!(
        shown(&outcome).message,
        Message::with("LangSet", vec![Text::Raw("English: Star Wars".into())])
    );
}

#[tokio::test]
async fn the_groups_language_is_a_config_option() {
    let store = Store::in_memory().await.unwrap();
    let chat = CommandChat {
        chat_id: GROUP,
        title: "Village".into(),
        username: None,
        is_group: true,
    };
    run_config_command(&store, &chat, admin(), 1).await.unwrap();
    let outcome = pick(&store, LanguageTarget::Group(GROUP), LanguageStep::OpenBases, admin()).await;
    assert_eq!(
        shown(&outcome).message,
        Message::with("WhatLang", vec![Text::Raw("English".into())])
    );
}

#[tokio::test]
async fn setting_the_groups_language_returns_to_the_config_menu() {
    let store = Store::in_memory().await.unwrap();
    let chat = CommandChat {
        chat_id: GROUP,
        title: "Village".into(),
        username: None,
        is_group: true,
    };
    run_config_command(&store, &chat, admin(), 1).await.unwrap();
    let outcome = pick(
        &store,
        LanguageTarget::Group(GROUP),
        LanguageStep::PickFile(LanguageId::of("English Star Wars")),
        admin(),
    )
    .await;
    assert_eq!(
        load_settings(&store, GROUP).await.unwrap().unwrap().language,
        "English Star Wars"
    );
    let CallbackOutcome::Show {
        screen,
        notice: Some(notice),
    } = outcome
    else {
        panic!("{outcome:?}")
    };
    assert_eq!(screen, main_menu(GROUP));
    assert_eq!(
        notice.message,
        Message::with("LangSet", vec![Text::Raw("English: Star Wars".into())])
    );
    assert!(!notice.alert);
}

#[tokio::test]
async fn non_admins_cannot_change_the_groups_language() {
    let store = Store::in_memory().await.unwrap();
    let chat = CommandChat {
        chat_id: GROUP,
        title: "Village".into(),
        username: None,
        is_group: true,
    };
    run_config_command(&store, &chat, admin(), 1).await.unwrap();
    let outcome = pick(
        &store,
        LanguageTarget::Group(GROUP),
        LanguageStep::PickFile(LanguageId::of("English Star Wars")),
        member(),
    )
    .await;
    assert_eq!(
        outcome,
        CallbackOutcome::Notify(Notice {
            message: Message::new("GroupAdminOnly"),
            alert: true
        })
    );
    assert_eq!(load_settings(&store, GROUP).await.unwrap().unwrap().language, "English");
}

#[test]
fn a_groups_language_defaults_to_english() {
    assert_eq!(GroupSettings::default().language, "English");
    assert!(choices().file(DEFAULT_LANGUAGE).is_some());
}

#[test]
fn every_language_button_fits_in_telegrams_callback_data() {
    let mut screens = vec![];
    for target in [LanguageTarget::Group(GROUP), LanguageTarget::Player(i64::MAX)] {
        screens.push(base_screen(choices(), target, "English"));
        for base in choices().bases() {
            let action = LanguageAction {
                target,
                step: LanguageStep::PickBase(LanguageId::of(base)),
            };
            if let (CallbackOutcome::Show { screen, .. }, None) =
                handle_language_callback(choices(), "English", &action, admin())
            {
                screens.push(screen);
            }
        }
    }
    let mut checked = 0;
    for button in screens.iter().flat_map(|s| s.rows.iter().flatten()) {
        let ButtonAction::Callback(action) = &button.action else {
            continue;
        };
        let data = action.encode();
        assert!(data.len() <= 64, "{data:?} is {} bytes", data.len());
        assert_eq!(Action::decode(&data).as_ref(), Ok(action));
        checked += 1;
    }
    assert!(checked > 200, "expected every base and file, checked {checked}");
}

#[test]
fn every_locale_key_the_picker_uses_exists_in_english_xml() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../Werewolf for Telegram/Languages/English.xml"
    );
    let pack = i18n::LanguagePack::load(path).unwrap();
    for key in all_locale_keys() {
        assert!(pack.get(key).is_some(), "{key} missing from English.xml");
    }
}

#[tokio::test]
async fn presses_for_an_unknown_player_or_group_are_ignored() {
    let store = Store::in_memory().await.unwrap();
    for target in [LanguageTarget::Player(PLAYER), LanguageTarget::Group(GROUP)] {
        let outcome = pick(
            &store,
            target,
            LanguageStep::PickFile(LanguageId::of("English")),
            admin(),
        )
        .await;
        assert_eq!(outcome, CallbackOutcome::Ignore);
    }
}

#[test]
fn a_file_name_too_long_for_the_legacy_buttons_is_pickable() {
    // 34 bytes: the legacy "preferred|<group>|<file>|toggle" button
    // overflowed with it. The id keeps every button the same size.
    let name = "Singapore (Public Housing Version)";
    let action = LanguageAction {
        target: LanguageTarget::Group(i64::MIN),
        step: LanguageStep::PickFile(LanguageId::of(name)),
    };
    assert!(action.encode().len() <= 64, "{}", action.encode());
    assert_eq!(
        choices().file_by_id(LanguageId::of(name)).map(|f| f.file.as_str()),
        Some(name)
    );
}

#[test]
fn a_button_for_a_removed_language_is_ignored() {
    for step in [
        LanguageStep::PickBase(LanguageId::of("Removed")),
        LanguageStep::PickFile(LanguageId::of("Removed")),
    ] {
        let action = LanguageAction {
            target: LanguageTarget::Player(PLAYER),
            step,
        };
        assert_eq!(
            handle_language_callback(choices(), "English", &action, member()),
            (CallbackOutcome::Ignore, None)
        );
    }
}

#[test]
fn names_in_button_data_are_rejected() {
    assert!(LanguageAction::decode("lng|p1|f|English").is_err());
}
