//! `features/group_config.feature`, one test per scenario (named after
//! it), driving `control::group_config` the way a presenter would:
//! commands and button presses in, screens out. Menu logic runs against
//! settings held in the test; the persistence scenarios go through a real
//! (in-memory SQLite) `db::Store`.

use control::group_config::*;
use control::language_picker::{LanguageAction, LanguageStep, LanguageTarget};
use control::lobby::{Candidate, ExtendError, Lobby};
use control::menu::*;
use db::Store;
use game_engine::rng::SeededRngSource;
use shared::{GameMode, Role};

const GROUP: i64 = -100123;

fn admin() -> Actor {
    Actor { can_configure: true }
}

fn member() -> Actor {
    Actor { can_configure: false }
}

fn group_chat(title: &str) -> CommandChat {
    CommandChat {
        chat_id: GROUP,
        title: title.to_string(),
        username: None,
        is_group: true,
    }
}

/// One group's settings as the menu sees them: `None` until `/config`
/// has recorded the group.
struct Group(Option<GroupSettings>);

/// A group already set up by an admin's `/config`.
fn configured() -> Group {
    Group(Some(GroupSettings::default()))
}

fn press(store: &mut Group, callback: Callback) -> CallbackOutcome {
    press_as(store, callback, admin())
}

fn press_as(store: &mut Group, callback: Callback, actor: Actor) -> CallbackOutcome {
    let mut rng = SeededRngSource::new(7);
    handle_callback(
        store.0.as_mut(),
        ConfigAction {
            group_id: GROUP,
            callback,
        },
        actor,
        &mut rng,
    )
}

fn screen(outcome: &CallbackOutcome) -> &Screen {
    match outcome {
        CallbackOutcome::Show { screen, .. } => screen,
        other => panic!("expected a screen, got {other:?}"),
    }
}

fn notice(outcome: &CallbackOutcome) -> &Notice {
    match outcome {
        CallbackOutcome::Show { notice: Some(n), .. } => n,
        CallbackOutcome::Notify(n) => n,
        other => panic!("expected a notice, got {other:?}"),
    }
}

fn settings(store: &Group) -> GroupSettings {
    store.0.clone().expect("group should have settings")
}

fn labels(screen: &Screen) -> Vec<Vec<String>> {
    screen
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|b| match &b.label {
                    Text::Locale(k) => k.clone(),
                    Text::Raw(s) => s.clone(),
                })
                .collect()
        })
        .collect()
}

fn flat_labels(screen: &Screen) -> Vec<String> {
    labels(screen).into_iter().flatten().collect()
}

fn callback_of(button: &Button) -> Callback {
    match &button.action {
        ButtonAction::Callback(Action::Config(a)) => a.callback,
        other => panic!("not a config button: {other:?}"),
    }
}

fn last_button(screen: &Screen) -> &Button {
    screen.rows.last().and_then(|r| r.last()).expect("screen has buttons")
}

fn is_submenu(screen: &Screen, submenu: Submenu) -> bool {
    let expected = submenu_screen(GROUP, &GroupSettings::default(), submenu);
    labels(screen) == labels(&expected) && screen.message == expected.message
}

// ---------- Defaults and the settings model ----------

#[tokio::test]
async fn a_group_that_has_never_been_configured_gets_the_legacy_defaults() {
    let db = Store::in_memory().await.unwrap();
    run_config_command(&db, &group_chat("Village Square"), admin(), 1)
        .await
        .unwrap();
    let s = load_settings(&db, GROUP)
        .await
        .unwrap()
        .expect("/config records the group");
    assert_eq!(s.mode, ModeSetting::Player);
    assert_eq!(s.max_players, 35);
    assert_eq!((s.day_seconds, s.night_seconds, s.lynch_seconds), (60, 90, 90));
    assert_eq!(s.max_extend_seconds, 60);
    assert_eq!(s.end_roles, EndRoles::All);
    for toggle in Toggle::ALL {
        let expected = matches!(toggle, Toggle::AllowFlee | Toggle::ShowRolesDeath);
        assert_eq!(s.is_on(toggle), expected, "{toggle:?}");
    }
    assert!(configurable_roles().all(|r| !s.is_role_disabled(r)));
    assert!(!s.roles_validated());
    assert_eq!(db.groups().get(GROUP).await.unwrap().unwrap().name, "Village Square");
}

#[tokio::test]
async fn running_config_again_refreshes_the_groups_name_but_keeps_its_settings() {
    let db = Store::in_memory().await.unwrap();
    let mut rng = SeededRngSource::new(1);
    run_config_command(&db, &group_chat("Village Square"), admin(), 1)
        .await
        .unwrap();
    let action = ConfigAction {
        group_id: GROUP,
        callback: Callback::SetMaxPlayers(20),
    };
    run_callback(&db, action, admin(), &mut rng, 2).await.unwrap();
    run_config_command(&db, &group_chat("Village Square II"), admin(), 3)
        .await
        .unwrap();
    assert_eq!(db.groups().get(GROUP).await.unwrap().unwrap().name, "Village Square II");
    assert_eq!(load_settings(&db, GROUP).await.unwrap().unwrap().max_players, 20);
}

// ---------- Who can use it ----------

#[test]
fn config_only_works_inside_a_group() {
    let chat = CommandChat {
        chat_id: 42,
        title: String::new(),
        username: None,
        is_group: false,
    };
    match config_command(&chat, admin()) {
        CommandOutcome::Refuse(m) => assert_eq!(m.key, "GroupCommandOnly"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn non_admins_cannot_open_the_config_menu() {
    let db = Store::in_memory().await.unwrap();
    match run_config_command(&db, &group_chat("Village Square"), member(), 1)
        .await
        .unwrap()
    {
        CommandOutcome::Refuse(m) => assert_eq!(m.key, "GroupAdminOnly"),
        other => panic!("{other:?}"),
    }
    assert!(load_settings(&db, GROUP).await.unwrap().is_none());
}

#[test]
fn a_non_admin_pressing_a_config_button_is_told_only_admins_can() {
    let mut store = configured();
    let before = settings(&store);
    let outcome = press_as(&mut store, Callback::SetMaxPlayers(5), member());
    assert_eq!(
        outcome,
        CallbackOutcome::Notify(Notice {
            message: notice(&outcome).message.clone(),
            alert: true
        })
    );
    assert_eq!(notice(&outcome).message.key, "GroupAdminOnly");
    assert_eq!(settings(&store), before);
}

#[test]
fn global_admins_can_configure_any_group() {
    // The caller folds group admin, global admin and developer into
    // `can_configure`; any of them reaches the submenu.
    let mut store = configured();
    let outcome = press(&mut store, Callback::OpenSubmenu(Submenu::Timers));
    assert!(is_submenu(screen(&outcome), Submenu::Timers));
}

// ---------- Menu structure ----------

#[test]
fn the_main_menu_is_sent_privately_to_the_admin() {
    let CommandOutcome::SendPrivately { menu, chat_notice, .. } =
        config_command(&group_chat("Village Square"), admin())
    else {
        panic!("expected the menu");
    };
    assert_eq!(menu.message.key, "WhatToDo");
    assert_eq!(chat_notice, None);
    assert_eq!(
        labels(&menu),
        vec![
            vec!["Timers", "RoleConfig"],
            vec!["Mechanics", "GroupSettings"],
            vec!["Done"]
        ]
    );
}

#[test]
fn if_the_bot_cannot_message_the_admin_privately_the_group_is_asked_to_start_a_pm() {
    let CommandOutcome::SendPrivately { fallback, .. } = config_command(&group_chat("Village Square"), admin()) else {
        panic!("expected the menu");
    };
    assert_eq!(fallback.message.key, "StartMe");
    assert_eq!(fallback.rows[0][0].action, ButtonAction::StartBotLink);
}

#[test]
fn each_submenu_lists_its_options_followed_by_back() {
    let cases = [
        (
            Submenu::Timers,
            vec!["MaxExtend", "DayTimer", "LynchTimer", "NightTimer"],
        ),
        (
            Submenu::Mechanics,
            vec![
                "EndRoles",
                "EnableSecretLynch",
                "RandomMode",
                "ShowRolesDeath",
                "ThiefFull",
                "SecretLynchShowVotes",
                "SecretLynchShowVoters",
            ],
        ),
        (
            Submenu::GroupSettings,
            vec![
                "Lang",
                "Mode",
                "MaxPlayer",
                "AllowExtend",
                "AllowFlee",
                "ShowIDs",
                "AllowNSFW",
                "ShufflePlayerList",
                "BurningOverkill",
            ],
        ),
    ];
    let mut store = configured();
    for (submenu, options) in cases {
        let outcome = press(&mut store, Callback::OpenSubmenu(submenu));
        let shown = flat_labels(screen(&outcome));
        let (back, rest) = shown.split_last().unwrap();
        if submenu == Submenu::GroupSettings {
            assert_eq!(
                screen(&outcome).rows[0][0].action,
                ButtonAction::Callback(Action::Language(LanguageAction {
                    target: LanguageTarget::Group(GROUP),
                    step: LanguageStep::OpenBases,
                }))
            );
        }
        assert_eq!(rest, options.as_slice(), "{submenu:?}");
        assert_eq!(back, "Back");
        assert_eq!(callback_of(last_button(screen(&outcome))), Callback::BackToMain);
        assert!(screen(&outcome).rows.iter().all(|r| r.len() <= 2));
    }
}

#[test]
fn done_closes_the_menu() {
    let mut store = configured();
    let outcome = press(&mut store, Callback::Done);
    assert_eq!(screen(&outcome).message.key, "ThankYou");
    assert!(screen(&outcome).rows.is_empty());
}

#[test]
fn cancel_on_any_option_returns_to_that_options_submenu_without_changing_anything() {
    let mut store = configured();
    let open = press(&mut store, Callback::OpenOption(ConfigOption::NightTimer));
    let cancel = callback_of(last_button(screen(&open)));
    assert_eq!(cancel, Callback::Cancel(ConfigOption::NightTimer));
    let outcome = press(&mut store, cancel);
    assert!(is_submenu(screen(&outcome), Submenu::Timers));
    assert_eq!(settings(&store).night_seconds, 90);
}

// ---------- Yes/no settings ----------

#[test]
fn a_yes_no_setting_shows_its_current_value_and_its_own_wording() {
    let cases = [
        (Toggle::AllowFlee, "Allow", ["Allow", "Disallow"]),
        (Toggle::EnableSecretLynch, "No", ["Yes", "No"]),
        (Toggle::ShowRolesDeath, "Show", ["Show", "Hide"]),
        (Toggle::BurningOverkill, "Disallow", ["Allow", "Disallow"]),
    ];
    let mut store = configured();
    for (toggle, current, [yes, no]) in cases {
        let outcome = press(&mut store, Callback::OpenOption(ConfigOption::Toggle(toggle)));
        let s = screen(&outcome);
        assert_eq!(s.message.key, format!("{}Q", toggle.name()));
        assert_eq!(s.message.args, vec![Text::Locale(current.to_string())]);
        assert_eq!(flat_labels(s), vec![yes, no, "Cancel"]);
    }
}

#[test]
fn changing_a_yes_no_setting_confirms_it_and_returns_to_its_submenu() {
    let mut store = configured();
    let outcome = press(&mut store, Callback::SetToggle(Toggle::EnableSecretLynch, true));
    assert!(settings(&store).is_on(Toggle::EnableSecretLynch));
    assert_eq!(notice(&outcome).message.key, "EnableSecretLynchA");
    assert_eq!(notice(&outcome).message.args, vec![Text::Locale("Yes".into())]);
    assert!(!notice(&outcome).alert);
    assert!(is_submenu(screen(&outcome), Submenu::Mechanics));
}

// ---------- Choice settings ----------

#[test]
fn choice_settings_offer_the_legacy_values() {
    let cases = [
        (ConfigOption::Mode, vec!["NormalOnly", "ChaosOnly", "PlayerChoice"]),
        (ConfigOption::EndRoles, vec!["ShowNone", "ShowLiving", "ShowAll"]),
        (ConfigOption::MaxPlayer, vec!["5", "10", "15", "20", "25", "30", "35"]),
        (ConfigOption::MaxExtend, vec!["60", "120", "180", "240", "300"]),
        (ConfigOption::NightTimer, vec!["30", "60", "90", "120"]),
        (ConfigOption::LynchTimer, vec!["30", "60", "90", "120"]),
    ];
    let mut store = configured();
    for (option, mut choices) in cases {
        choices.push("Cancel");
        let outcome = press(&mut store, Callback::OpenOption(option));
        assert_eq!(flat_labels(screen(&outcome)), choices, "{option:?}");
    }
}

#[test]
fn every_choice_button_stores_its_value_and_returns_to_its_submenu() {
    let mut store = configured();
    for option in [
        ConfigOption::Mode,
        ConfigOption::EndRoles,
        ConfigOption::MaxExtend,
        ConfigOption::LynchTimer,
    ] {
        let open = press(&mut store, Callback::OpenOption(option));
        let first = callback_of(&screen(&open).rows[0][0]);
        let outcome = press(&mut store, first);
        assert!(is_submenu(screen(&outcome), option.submenu()), "{option:?}");
    }
    let s = settings(&store);
    assert_eq!(s.mode, ModeSetting::Normal);
    assert_eq!(s.end_roles, EndRoles::None);
    assert_eq!(s.max_extend_seconds, 60);
    assert_eq!(s.lynch_seconds, 30);
}

#[test]
fn the_day_timer_keeps_the_legacy_60_second_offset_between_stored_and_shown_values() {
    let mut store = configured();
    let open = press(&mut store, Callback::OpenOption(ConfigOption::DayTimer));
    let s = screen(&open);
    assert_eq!(flat_labels(s), vec!["90", "120", "150", "180", "Cancel"]);
    assert_eq!(s.message.args, vec![Text::Raw("120".into()), Text::Raw("120".into())]);
    let outcome = press(&mut store, callback_of(&s.rows[2][0]));
    assert_eq!(settings(&store).day_seconds, 90);
    assert_eq!(notice(&outcome).message.key, "SetDayTimeA");
    assert_eq!(notice(&outcome).message.args, vec![Text::Raw("150".into())]);
}

#[test]
fn the_game_mode_setting_decides_which_modes_a_game_can_start_in() {
    let mut s = GroupSettings::default();
    s.mode = ModeSetting::Normal;
    assert_eq!(
        Lobby::from_group_settings(&s, GameMode::Chaos).game_mode(),
        GameMode::Normal
    );
    s.mode = ModeSetting::Player;
    assert_eq!(
        Lobby::from_group_settings(&s, GameMode::Chaos).game_mode(),
        GameMode::Chaos
    );
    assert_eq!(
        Lobby::from_group_settings(&s, GameMode::Normal).game_mode(),
        GameMode::Normal
    );
}

// ---------- Role configuration ----------

#[test]
fn the_role_menu_shows_every_role_that_can_be_disabled_three_per_row() {
    let mut store = configured();
    let outcome = press(&mut store, Callback::OpenSubmenu(Submenu::RoleConfig));
    let s = screen(&outcome);
    assert_eq!(s.message.key, "RoleConfigInfo");

    let toggles: Vec<&Button> = s.rows[..s.rows.len() - 2].iter().flatten().collect();
    assert_eq!(
        toggles.len(),
        40,
        "the 43 legacy roles minus Villager, Wolf and Spumpkin"
    );
    for role in [
        Role::Villager,
        Role::Wolf,
        Role::Spumpkin,
        Role::Chef,
        Role::Barkeep,
        Role::Witch,
    ] {
        assert!(
            !toggles.iter().any(|b| callback_of(b) == Callback::ToggleRole(role)),
            "{role:?}"
        );
    }
    assert_eq!(toggles[0].label, Text::Raw("🍻 ✅".into()));
    assert!(s.rows[..s.rows.len() - 2].iter().all(|r| r.len() <= 3));

    let tail: Vec<Vec<String>> = labels(s).split_off(s.rows.len() - 2);
    assert_eq!(
        tail,
        vec![vec!["EnableAllRoles", "DisableAllRoles"], vec!["Validate", "Back"]]
    );
}

#[test]
fn toggling_a_role_disables_it_and_clears_validation() {
    let mut store = configured();
    press(&mut store, Callback::EnableAllRoles);
    assert!(settings(&store).roles_validated());
    let outcome = press(&mut store, Callback::ToggleRole(Role::Seer));
    assert!(settings(&store).is_role_disabled(Role::Seer));
    assert!(!settings(&store).roles_validated());
    assert!(flat_labels(screen(&outcome)).contains(&"👳 ❌".to_string()));
}

#[test]
fn disabling_all_roles_leaves_only_the_roles_that_cannot_be_disabled() {
    let mut store = configured();
    press(&mut store, Callback::DisableAllRoles);
    let s = settings(&store);
    assert!(configurable_roles().all(|r| s.is_role_disabled(r)));
    for role in [Role::Villager, Role::Wolf, Role::Spumpkin] {
        assert!(!s.is_role_disabled(role));
    }
    assert!(!s.roles_validated());
}

#[test]
fn enabling_all_roles_is_automatically_valid() {
    let mut store = configured();
    press(&mut store, Callback::DisableAllRoles);
    let outcome = press(&mut store, Callback::EnableAllRoles);
    let s = settings(&store);
    assert!(configurable_roles().all(|r| !s.is_role_disabled(r)));
    assert!(s.roles_validated());
    assert_eq!(
        callback_of(&screen(&outcome).rows.last().unwrap()[0]),
        Callback::Nothing
    );
}

#[test]
fn validation_passes_when_every_player_count_from_5_to_max_players_can_be_balanced() {
    let mut store = configured();
    press(&mut store, Callback::SetMaxPlayers(15));
    press(&mut store, Callback::ToggleRole(Role::Cultist));
    press(&mut store, Callback::ToggleRole(Role::SerialKiller));
    let outcome = press(&mut store, Callback::ValidateRoles);
    assert!(settings(&store).roles_validated());
    assert_eq!(
        notice(&outcome),
        &Notice {
            message: notice(&outcome).message.clone(),
            alert: true
        }
    );
    assert_eq!(notice(&outcome).message.key, "RoleConfigValid");
    let validate_row = screen(&outcome).rows.last().unwrap();
    assert_eq!(validate_row[0].label, Text::Locale("Valid".into()));
}

#[test]
fn validation_fails_when_some_player_count_cannot_be_balanced() {
    let mut store = configured();
    press(&mut store, Callback::DisableAllRoles);
    let outcome = press(&mut store, Callback::ValidateRoles);
    assert!(!settings(&store).roles_validated());
    assert_eq!(notice(&outcome).message.key, "RoleConfigInvalid");
}

#[test]
fn disabled_roles_only_affect_games_once_validated() {
    let mut s = GroupSettings::default();
    s.toggle_role(Role::Seer);
    assert!(Lobby::from_group_settings(&s, GameMode::Normal)
        .disabled_roles()
        .is_empty());
    assert!(s.validate_roles(&mut SeededRngSource::new(1)));
    assert_eq!(
        Lobby::from_group_settings(&s, GameMode::Normal).disabled_roles(),
        &[Role::Seer]
    );
}

#[test]
fn raising_max_players_clears_validation_lowering_it_does_not() {
    let mut store = configured();
    press(&mut store, Callback::SetMaxPlayers(20));
    press(&mut store, Callback::EnableAllRoles);
    press(&mut store, Callback::SetMaxPlayers(25));
    assert!(!settings(&store).roles_validated());
    press(&mut store, Callback::EnableAllRoles);
    press(&mut store, Callback::SetMaxPlayers(10));
    assert!(settings(&store).roles_validated());
}

// ---------- Settings that reach the game today ----------

#[test]
fn settings_the_ported_lobby_already_understands_come_from_the_group() {
    let mut s = GroupSettings::default();
    s.set_max_players(20);
    s.max_extend_seconds = 120;
    s.set(Toggle::BurningOverkill, true);
    s.set(Toggle::AllowExtend, true);
    s.mode = ModeSetting::Chaos;

    let mut lobby = Lobby::from_group_settings(&s, GameMode::Normal);
    assert_eq!(lobby.max_players(), 20);
    assert_eq!(lobby.game_mode(), GameMode::Chaos);
    assert!(lobby.burning_overkill());

    lobby
        .join(Candidate {
            telegram_id: 1,
            name: "Alice".into(),
        })
        .unwrap();
    assert_eq!(lobby.extend_time(1, false, 999), Ok(120));
}

#[test]
fn disallowing_extend_blocks_extend_for_non_admins() {
    let s = GroupSettings::default();
    assert!(!s.is_on(Toggle::AllowExtend), "legacy default is off");
    let mut lobby = Lobby::from_group_settings(&s, GameMode::Normal);
    lobby
        .join(Candidate {
            telegram_id: 1,
            name: "Alice".into(),
        })
        .unwrap();
    assert_eq!(lobby.extend_time(1, false, 30), Err(ExtendError::ExtendDisabled));
    assert_eq!(lobby.extend_time(1, true, 30), Ok(30));
}

#[test]
fn disallowing_flee_blocks_flee_once_the_game_is_running() {
    let mut s = GroupSettings::default();
    assert!(s.flee_allowed_while_running());
    s.set(Toggle::AllowFlee, false);
    assert!(!s.flee_allowed_while_running());
    assert_eq!(FLEE_DISABLED_KEY, "FleeDisabled");
}

// ---------- Plumbing ----------

#[test]
fn every_button_on_every_screen_round_trips_through_callback_data() {
    let mut s = GroupSettings::default();
    s.toggle_role(Role::Seer);
    let mut screens = vec![main_menu(GROUP), role_menu(GROUP, &s)];
    for submenu in Submenu::ALL {
        screens.push(submenu_screen(GROUP, &s, submenu));
        for option in submenu_options(submenu) {
            screens.push(option_screen(GROUP, &s, option));
        }
    }
    for button in screens.iter().flat_map(|s| s.rows.iter().flatten()) {
        let ButtonAction::Callback(action) = &button.action else {
            continue;
        };
        let data = action.encode();
        assert!(data.len() <= 64, "{data} is over Telegram's 64-byte limit");
        assert_eq!(Action::decode(&data).as_ref(), Ok(action), "{data}");
    }
    assert!(Action::decode("cfg|1|zz").is_err());
    assert!(Action::decode("vote|1|2").is_err());
}

#[test]
fn every_locale_key_the_menu_uses_exists_in_english_xml() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../Werewolf for Telegram/Languages/English.xml"
    );
    let pack = i18n::LanguagePack::load(path).expect("English.xml should load");
    let missing: Vec<String> = all_locale_keys()
        .into_iter()
        .filter(|k| pack.get(k).is_none())
        .collect();
    assert!(missing.is_empty(), "missing from English.xml: {missing:?}");
}

#[tokio::test]
async fn presses_for_a_group_that_never_ran_config_are_ignored() {
    let db = Store::in_memory().await.unwrap();
    let mut rng = SeededRngSource::new(1);
    let action = ConfigAction {
        group_id: GROUP,
        callback: Callback::OpenSubmenu(Submenu::Timers),
    };
    assert_eq!(
        run_callback(&db, action, admin(), &mut rng, 1).await.unwrap(),
        CallbackOutcome::Ignore
    );
    let done = ConfigAction {
        group_id: GROUP,
        callback: Callback::Done,
    };
    assert!(matches!(
        run_callback(&db, done, admin(), &mut rng, 1).await.unwrap(),
        CallbackOutcome::Show { .. }
    ));
}

#[tokio::test]
async fn changed_settings_survive_a_reload_and_role_validation_is_kept() {
    let db = Store::in_memory().await.unwrap();
    let mut rng = SeededRngSource::new(1);
    run_config_command(&db, &group_chat("Village Square"), admin(), 1)
        .await
        .unwrap();
    for callback in [
        Callback::SetToggle(Toggle::AllowExtend, true),
        Callback::SetNightTimer(60),
        Callback::ToggleRole(Role::Seer),
        Callback::ValidateRoles,
    ] {
        run_callback(
            &db,
            ConfigAction {
                group_id: GROUP,
                callback,
            },
            admin(),
            &mut rng,
            2,
        )
        .await
        .unwrap();
    }
    let s = load_settings(&db, GROUP).await.unwrap().unwrap();
    assert!(s.is_on(Toggle::AllowExtend));
    assert_eq!(s.night_seconds, 60);
    assert!(s.is_role_disabled(Role::Seer));
    assert!(s.roles_validated());
}

#[test]
fn a_stored_document_missing_newer_settings_reads_back_with_their_defaults() {
    let stored = serde_json::json!({ "max_players": 20, "enabled": ["AllowExtend"] });
    let s: GroupSettings = serde_json::from_value(stored).unwrap();
    assert_eq!(s.max_players, 20);
    assert!(s.is_on(Toggle::AllowExtend));
    assert_eq!(s.night_seconds, 90);
    assert_eq!(s.mode, ModeSetting::Player);
}

// ---------- Group link (/setlink, /remlink) ----------

async fn configured_db() -> Store {
    let db = Store::in_memory().await.unwrap();
    run_config_command(&db, &group_chat("Village Square"), admin(), 1)
        .await
        .unwrap();
    db
}

async fn link_of(db: &Store) -> Option<String> {
    load_settings(db, GROUP).await.unwrap().unwrap().link
}

fn reply(outcome: CommandOutcome) -> String {
    match outcome {
        CommandOutcome::Reply(text) => text,
        other => panic!("expected a reply, got {other:?}"),
    }
}

#[tokio::test]
async fn an_admin_sets_the_groups_invite_link() {
    let db = configured_db().await;
    let out = run_setlink_command(
        &db,
        &group_chat("Village <Square>"),
        admin(),
        Some(" https://t.me/+AbC_12-x "),
        2,
    )
    .await
    .unwrap();
    assert_eq!(link_of(&db).await.as_deref(), Some("https://t.me/+AbC_12-x"));
    assert_eq!(
        reply(out),
        "Link set: <a href=\"https://t.me/+AbC_12-x\">Village &lt;Square&gt;</a>"
    );
}

#[tokio::test]
async fn a_link_set_before_config_is_kept() {
    let db = Store::in_memory().await.unwrap();
    run_setlink_command(&db, &group_chat("Village Square"), admin(), Some("t.me/+abc"), 1)
        .await
        .unwrap();
    assert_eq!(link_of(&db).await.as_deref(), Some("t.me/+abc"));
}

#[test]
fn only_telegram_join_links_are_accepted() {
    for ok in [
        "t.me/joinchat/AbC123",
        "http://telegram.me/+abc",
        "https://t.me/+AbC_12-x",
    ] {
        assert!(is_invite_link(ok), "{ok}");
    }
    for bad in [
        "https://t.me/somegroup",
        "https://example.com/+abc",
        "https://t.me/+abc?start=1",
        "https://t.me/+",
        "",
    ] {
        assert!(!is_invite_link(bad), "{bad}");
    }
}

#[tokio::test]
async fn setlink_without_a_link_explains_how_to_use_it() {
    let db = configured_db().await;
    for arg in [None, Some("  ")] {
        let out = run_setlink_command(&db, &group_chat("Village Square"), admin(), arg, 2)
            .await
            .unwrap();
        assert_eq!(
            reply(out),
            "You must use /setlink with the link to the group (invite link)"
        );
    }
    let out = run_setlink_command(&db, &group_chat("Village Square"), admin(), Some("https://t.me/x"), 2)
        .await
        .unwrap();
    assert_eq!(reply(out), "This is an invalid telegram join link.");
    assert_eq!(link_of(&db).await, None);
}

#[tokio::test]
async fn a_public_group_already_has_a_link() {
    let db = configured_db().await;
    let chat = CommandChat {
        username: Some("villagesquare".into()),
        ..group_chat("Village Square")
    };
    let out = run_setlink_command(&db, &chat, admin(), Some("https://t.me/+abc"), 2)
        .await
        .unwrap();
    assert_eq!(
        reply(out),
        "Your group link has already been set to https://telegram.me/villagesquare"
    );
    assert_eq!(link_of(&db).await, None);
}

#[tokio::test]
async fn an_admin_removes_the_groups_link() {
    let db = configured_db().await;
    run_setlink_command(
        &db,
        &group_chat("Village Square"),
        admin(),
        Some("https://t.me/+abc"),
        2,
    )
    .await
    .unwrap();
    let out = run_remlink_command(&db, &group_chat("Village Square"), admin(), 3)
        .await
        .unwrap();
    assert_eq!(link_of(&db).await, None);
    assert_eq!(reply(out), "Your group link has been removed.");
}

#[tokio::test]
async fn only_admins_can_change_the_link_and_only_in_a_group() {
    let db = configured_db().await;
    let private = CommandChat {
        is_group: false,
        ..group_chat("")
    };
    for (chat, actor, key) in [
        (group_chat("Village Square"), member(), "GroupAdminOnly"),
        (private, admin(), "GroupCommandOnly"),
    ] {
        let set = run_setlink_command(&db, &chat, actor, Some("https://t.me/+abc"), 2)
            .await
            .unwrap();
        let rem = run_remlink_command(&db, &chat, actor, 2).await.unwrap();
        for out in [set, rem] {
            assert_eq!(
                out,
                CommandOutcome::Refuse(Message {
                    key: key.into(),
                    args: vec![]
                })
            );
        }
    }
    assert_eq!(link_of(&db).await, None);
}

#[test]
fn the_you_joined_message_links_the_groups_name() {
    assert_eq!(
        linked_group_name("A & B", Some("https://t.me/+abc")),
        "<a href=\"https://t.me/+abc\">A &amp; B</a>"
    );
    assert_eq!(linked_group_name("A & B", None), "A & B");
}
