//! Choosing a language: the per-player `/setlang` (`GeneralCommands.cs:
//! 112-185`) and the group's `Lang` config option (`UpdateHandler.cs:
//! 1408-1500`), which share one two-step picker (`SelectLanguage`,
//! `UpdateHandler.cs:2071-2105`): pick a base language, then a variant
//! when the base has more than one file.
//!
//! A language is identified by its file's stem (`English`, `English
//! Pirate`, ...), as the legacy `Group.Language`/`Player.Language` columns
//! do. The group's choice lives in `GroupSettings::language`; a player's
//! in `db::Players`.
//!
//! The legacy "Random" variant is not ported: its button is commented
//! out (`UpdateHandler.cs:2081`, "TO DO: Publish Random Language Variant
//! Mode"), so no user could ever pick it.

use crate::group_config::{load_settings, main_menu, GroupSettings};
use crate::menu::{
    rows_of, start_me_prompt, Action, Actor, Button, ButtonAction, CallbackOutcome, CommandOutcome, DecodeError, Message,
    Notice, Screen, Text,
};
use db::{DbResult, Store};
use i18n::Catalog;

/// What a group or player without a choice speaks (`Helpers.cs:283,302`).
pub const DEFAULT_LANGUAGE: &str = "English";

/// One language file's `<language>` metadata (`LanguageHelper.cs::LangFile`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageFile {
    /// The file name without `.xml`: what groups and players store.
    pub file: String,
    pub base: String,
    pub variant: String,
    pub is_default: bool,
}

impl LanguageFile {
    /// `"Base: Variant"`, or just the base when the file has no variant
    /// (`UpdateHandler.cs:1491`).
    pub fn display_name(&self) -> String {
        if self.variant.trim().is_empty() {
            self.base.clone()
        } else {
            format!("{}: {}", self.base, self.variant)
        }
    }
}

/// The languages on offer.
#[derive(Debug, Clone, Default)]
pub struct LanguageChoices {
    files: Vec<LanguageFile>,
}

impl LanguageChoices {
    pub fn new(files: Vec<LanguageFile>) -> Self {
        LanguageChoices { files }
    }

    pub fn from_catalog(catalog: &Catalog) -> Self {
        Self::new(
            catalog
                .iter()
                .map(|(file, pack)| LanguageFile {
                    file: file.to_string(),
                    base: pack.meta.base.clone(),
                    variant: pack.meta.variant.clone(),
                    is_default: pack.meta.is_default == Some(true),
                })
                .collect(),
        )
    }

    pub fn file(&self, file: &str) -> Option<&LanguageFile> {
        self.files.iter().find(|f| f.file == file)
    }

    /// Every base language once, sorted (`GeneralCommands.cs:163`).
    pub fn bases(&self) -> Vec<&str> {
        let mut bases: Vec<&str> = self.files.iter().map(|f| f.base.as_str()).collect();
        bases.sort();
        bases.dedup();
        bases
    }

    /// A base's files, the default first, then by variant name
    /// (`UpdateHandler.cs:2077`).
    pub fn variants(&self, base: &str) -> Vec<&LanguageFile> {
        let mut variants: Vec<&LanguageFile> = self.files.iter().filter(|f| f.base == base).collect();
        variants.sort_by(|a, b| b.is_default.cmp(&a.is_default).then_with(|| a.variant.cmp(&b.variant)));
        variants
    }
}

/// Whose language is being chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LanguageTarget {
    Group(i64),
    Player(i64),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LanguageStep {
    /// Show the base list (the group config's `Lang` button).
    OpenBases,
    PickBase(String),
    PickFile(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LanguageAction {
    pub target: LanguageTarget,
    pub step: LanguageStep,
}

fn button(target: LanguageTarget, label: String, step: LanguageStep) -> Button {
    Button { label: Text::Raw(label), action: ButtonAction::Callback(Action::Language(LanguageAction { target, step })) }
}

/// The group config's `Lang` button.
pub fn group_language_button(group_id: i64) -> Button {
    Button {
        label: Text::key("Lang"),
        action: ButtonAction::Callback(Action::Language(LanguageAction {
            target: LanguageTarget::Group(group_id),
            step: LanguageStep::OpenBases,
        })),
    }
}

/// `WhatLang` with the current base, and every base two per row.
pub fn base_screen(choices: &LanguageChoices, target: LanguageTarget, current: &str) -> Screen {
    let current_base = choices.file(current).map(|f| f.base.clone()).unwrap_or_else(|| current.to_string());
    let buttons = choices
        .bases()
        .into_iter()
        .map(|b| button(target, b.to_string(), LanguageStep::PickBase(b.to_string())))
        .collect();
    Screen { message: Message::with("WhatLang", vec![Text::Raw(current_base)]), rows: rows_of(buttons, 2) }
}

/// `WhatVariant` with the *current* language's variant (as the legacy
/// prompt does, whichever base was picked), and the base's files two per
/// row.
fn variant_screen(choices: &LanguageChoices, target: LanguageTarget, base: &str, current: &str) -> Screen {
    let current_variant = choices.file(current).map(|f| f.variant.clone()).unwrap_or_default();
    let buttons = choices
        .variants(base)
        .into_iter()
        .map(|f| button(target, f.variant.clone(), LanguageStep::PickFile(f.file.clone())))
        .collect();
    Screen { message: Message::with("WhatVariant", vec![Text::Raw(current_variant)]), rows: rows_of(buttons, 2) }
}

/// A picker button press. Returns what to show and, when the press chose
/// a language, the file to store.
pub fn handle_language_callback(
    choices: &LanguageChoices,
    current: &str,
    action: &LanguageAction,
    actor: Actor,
) -> (CallbackOutcome, Option<String>) {
    // The group's language is a config setting (UpdateHandler.cs:1031).
    if matches!(action.target, LanguageTarget::Group(_)) && !actor.can_configure {
        return (CallbackOutcome::Notify(Notice { message: Message::new("GroupAdminOnly"), alert: true }), None);
    }

    let chosen = match &action.step {
        LanguageStep::OpenBases => {
            let screen = base_screen(choices, action.target, current);
            return (CallbackOutcome::Show { screen, notice: None }, None);
        }
        LanguageStep::PickBase(base) => {
            let variants = choices.variants(base);
            match variants.as_slice() {
                [] => return (CallbackOutcome::Ignore, None),
                [only] => *only,
                _ => {
                    let screen = variant_screen(choices, action.target, base, current);
                    return (CallbackOutcome::Show { screen, notice: None }, None);
                }
            }
        }
        LanguageStep::PickFile(file) => match choices.file(file) {
            Some(f) => f,
            None => return (CallbackOutcome::Ignore, None),
        },
    };

    let lang_set = Message::with("LangSet", vec![Text::Raw(chosen.display_name())]);
    let outcome = match action.target {
        // UpdateHandler.cs:1487-1492: confirm, then back to the config menu.
        LanguageTarget::Group(group_id) => CallbackOutcome::Show {
            screen: main_menu(group_id),
            notice: Some(Notice { message: lang_set, alert: false }),
        },
        // UpdateHandler.cs:1494-1497: the message becomes the confirmation.
        LanguageTarget::Player(_) => CallbackOutcome::Show { screen: Screen { message: lang_set, rows: vec![] }, notice: None },
    };
    (outcome, Some(chosen.file.clone()))
}

/// Who sent `/setlang`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandUser {
    pub telegram_id: i64,
    pub name: String,
    pub username: Option<String>,
}

/// `/setlang`: records the player (`GeneralCommands.cs:115-146`) and
/// sends them the base list privately, telling the group if it came from
/// one (`SentPrivate`).
pub async fn run_setlang_command(
    store: &Store,
    choices: &LanguageChoices,
    user: &CommandUser,
    in_private_chat: bool,
    now: i64,
) -> DbResult<CommandOutcome> {
    store.players().upsert(user.telegram_id, &user.name, user.username.as_deref(), now).await?;
    let current = player_language(store, user.telegram_id).await?;
    Ok(CommandOutcome::SendPrivately {
        menu: base_screen(choices, LanguageTarget::Player(user.telegram_id), &current),
        chat_notice: (!in_private_chat).then(|| Message::new("SentPrivate")),
        fallback: start_me_prompt(),
    })
}

/// A player's language, or English (`Helpers.cs::GetLanguage`).
pub async fn player_language(store: &Store, telegram_id: i64) -> DbResult<String> {
    Ok(store
        .players()
        .get(telegram_id)
        .await?
        .and_then(|p| p.language)
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| DEFAULT_LANGUAGE.to_string()))
}

/// [`handle_language_callback`] against `store`, saving the choice.
pub async fn run_language_callback(
    store: &Store,
    choices: &LanguageChoices,
    action: &LanguageAction,
    actor: Actor,
    now: i64,
) -> DbResult<CallbackOutcome> {
    match action.target {
        LanguageTarget::Group(group_id) => {
            let Some(record) = store.groups().get(group_id).await? else {
                return Ok(CallbackOutcome::Ignore);
            };
            let mut settings: GroupSettings = load_settings(store, group_id).await?.unwrap_or_default();
            let (outcome, chosen) = handle_language_callback(choices, &settings.language, action, actor);
            if let Some(file) = chosen {
                settings.language = file;
                store.groups().put_settings(group_id, &record.name, &settings, now).await?;
            }
            Ok(outcome)
        }
        LanguageTarget::Player(player_id) => {
            if store.players().get(player_id).await?.is_none() {
                return Ok(CallbackOutcome::Ignore);
            }
            let current = player_language(store, player_id).await?;
            let (outcome, chosen) = handle_language_callback(choices, &current, action, actor);
            if let Some(file) = chosen {
                store.players().set_language(player_id, Some(&file), now).await?;
            }
            Ok(outcome)
        }
    }
}

impl LanguageAction {
    /// `"lng|<g|p><id>|<o|b|f>[|<name>]"`. Base and file names are at most
    /// 34 bytes in the shipped language files, which keeps the longest
    /// action under Telegram's 64-byte limit.
    pub fn encode(&self) -> String {
        let target = match self.target {
            LanguageTarget::Group(id) => format!("g{id}"),
            LanguageTarget::Player(id) => format!("p{id}"),
        };
        match &self.step {
            LanguageStep::OpenBases => format!("lng|{target}|o"),
            LanguageStep::PickBase(base) => format!("lng|{target}|b|{base}"),
            LanguageStep::PickFile(file) => format!("lng|{target}|f|{file}"),
        }
    }

    pub fn decode(data: &str) -> Result<Self, DecodeError> {
        let err = || DecodeError(data.to_string());
        let mut parts = data.splitn(4, '|');
        if parts.next() != Some("lng") {
            return Err(err());
        }
        let target = parts.next().ok_or_else(err)?;
        let id = |s: &str| s.parse::<i64>().map_err(|_| err());
        let target = if let Some(rest) = target.strip_prefix('g') {
            LanguageTarget::Group(id(rest)?)
        } else if let Some(rest) = target.strip_prefix('p') {
            LanguageTarget::Player(id(rest)?)
        } else {
            return Err(err());
        };
        let step = match (parts.next(), parts.next()) {
            (Some("o"), None) => LanguageStep::OpenBases,
            (Some("b"), Some(base)) => LanguageStep::PickBase(base.to_string()),
            (Some("f"), Some(file)) => LanguageStep::PickFile(file.to_string()),
            _ => return Err(err()),
        };
        Ok(LanguageAction { target, step })
    }
}

/// Every locale key the picker can emit.
pub fn all_locale_keys() -> Vec<&'static str> {
    vec!["Lang", "WhatLang", "WhatVariant", "LangSet", "SentPrivate", "GroupAdminOnly", "StartMe"]
}
