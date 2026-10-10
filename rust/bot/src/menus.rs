//! The menu commands `control` implements Telegram-free (`/config`,
//! `/setlang`, `/setlink`, `/remlink`), drawn here: their `Screen`s become
//! messages with inline keyboards and their callback data round-trips
//! through `control::menu::Action`. Plus `/getlang`, which sends language
//! files (GeneralCommands.cs:435-500).

use crate::core::BotCore;
use crate::game::unix_now;
use crate::telegram::{Button, Command, Keyboard, MsgRef, SendError, User};
use control::group_config::{run_callback, run_config_command, run_remlink_command, run_setlink_command, CommandChat};
use control::language_picker::{
    player_language, run_language_callback, run_setlang_command, CommandUser, LanguageTarget,
};
use control::menu::{Action, Actor, ButtonAction, CallbackOutcome, CommandOutcome, Message, Screen, Text};
use game_engine::rng::SeededRngSource;
use i18n::LanguageId;
use moderation::CommandPolicy;

/// Gate policies for the commands `moderation::policy_for` doesn't list.
/// `/setlink` and `/remlink` are `[Command(Trigger = ..., GroupAdminOnly =
/// true, InGroupOnly = true)]` in AdminCommands.cs:273, 300.
pub fn policy_for(trigger: &str) -> Option<CommandPolicy> {
    match trigger {
        "setlink" | "remlink" => Some(CommandPolicy {
            group_admin_only: true,
            in_group_only: true,
            allow_anonymous_admins: true,
            ..CommandPolicy::default()
        }),
        _ => None,
    }
}

impl BotCore {
    fn render_text(&self, lang: &str, text: &Text) -> String {
        match text {
            Text::Locale(key) => self.texts().key(lang, key, &[]),
            Text::Raw(raw) => raw.clone(),
        }
    }

    pub(crate) fn render_message(&self, lang: &str, message: &Message) -> String {
        let args: Vec<String> = message.args.iter().map(|a| self.render_text(lang, a)).collect();
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        self.texts().key(lang, &message.key, &args)
    }

    fn render_keyboard(&self, lang: &str, screen: &Screen) -> Option<Keyboard> {
        if screen.rows.is_empty() {
            return None;
        }
        Some(
            screen
                .rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|b| {
                            let text = self.render_text(lang, &b.label);
                            match &b.action {
                                ButtonAction::Callback(action) => Button::Callback {
                                    text,
                                    data: action.encode(),
                                },
                                ButtonAction::StartBotLink => Button::Url {
                                    text,
                                    url: format!("https://t.me/{}?start=config", self.telegram.bot_username()),
                                },
                            }
                        })
                        .collect()
                })
                .collect(),
        )
    }

    async fn show(&self, chat: i64, lang: &str, screen: &Screen) -> Result<MsgRef, SendError> {
        self.telegram
            .send(
                chat,
                &self.render_message(lang, &screen.message),
                self.render_keyboard(lang, screen),
            )
            .await
    }

    async fn can_configure(&self, group: i64, user: i64) -> bool {
        self.is_global_admin(user) || self.telegram.is_group_admin(group, user).await.unwrap_or(false)
    }

    async fn play_out(&self, cmd: &Command, lang: &str, outcome: CommandOutcome) {
        match outcome {
            CommandOutcome::SendPrivately {
                menu,
                chat_notice,
                fallback,
            } => match self.show(cmd.from.id, lang, &menu).await {
                Err(SendError::Unreachable) => {
                    let _ = self.show(cmd.chat.id, lang, &fallback).await;
                }
                _ => {
                    if let Some(notice) = chat_notice {
                        self.say(cmd.chat.id, &self.render_message(lang, &notice)).await;
                    }
                }
            },
            CommandOutcome::Refuse(message) => self.say(cmd.chat.id, &self.render_message(lang, &message)).await,
            CommandOutcome::Reply(html) => self.say(cmd.chat.id, &html).await,
        }
    }

    pub(crate) async fn menu_command(&self, cmd: &Command, lang: &str) {
        let now = unix_now();
        let chat = CommandChat {
            chat_id: cmd.chat.id,
            title: cmd.chat.title().to_string(),
            username: None,
            is_group: cmd.chat.is_group(),
        };
        let actor = || async {
            Actor {
                can_configure: cmd.chat.is_group() && self.can_configure(cmd.chat.id, cmd.from.id).await,
            }
        };
        let outcome = match cmd.name.as_str() {
            "config" => run_config_command(&self.store, &chat, actor().await, now).await,
            "setlink" => {
                let link = Some(cmd.args.as_str()).filter(|a| !a.is_empty());
                run_setlink_command(&self.store, &chat, actor().await, link, now).await
            }
            "remlink" => run_remlink_command(&self.store, &chat, actor().await, now).await,
            "setlang" => {
                let user = CommandUser {
                    telegram_id: cmd.from.id,
                    name: cmd.from.name.clone(),
                    username: cmd.from.username.clone(),
                };
                let player_lang = player_language(&self.store, cmd.from.id)
                    .await
                    .unwrap_or_else(|_| lang.to_string());
                let outcome =
                    run_setlang_command(&self.store, &self.texts().choices(), &user, !cmd.chat.is_group(), now).await;
                if let Ok(o) = outcome {
                    self.play_out(cmd, &player_lang, o).await;
                }
                return;
            }
            "getlang" => {
                self.getlang(cmd, lang).await;
                return;
            }
            _ => return,
        };
        match outcome {
            Ok(o) => self.play_out(cmd, lang, o).await,
            Err(e) => log::error!("/{}: {e}", cmd.name),
        }
    }

    /// Handles `cfg|...`, `lng|...` and `gl|...` button presses. Returns
    /// false for data that isn't a menu's.
    pub(crate) async fn menu_callback(&self, id: &str, from: &User, message: Option<MsgRef>, data: &str) -> bool {
        if let Some(rest) = data.strip_prefix("gl|") {
            self.getlang_callback(id, from, rest).await;
            return true;
        }
        let Ok(action) = Action::decode(data) else {
            return false;
        };
        let now = unix_now();
        let (outcome, lang) = match &action {
            Action::Config(a) => {
                let actor = Actor {
                    can_configure: self.can_configure(a.group_id, from.id).await,
                };
                let mut rng = SeededRngSource::new(rand::random());
                let lang = self.settings(a.group_id).await.language;
                (run_callback(&self.store, *a, actor, &mut rng, now).await, lang)
            }
            Action::Language(a) => {
                let (can_configure, lang) = match a.target {
                    LanguageTarget::Group(g) => (self.can_configure(g, from.id).await, self.settings(g).await.language),
                    LanguageTarget::Player(_) => (
                        false,
                        player_language(&self.store, from.id)
                            .await
                            .unwrap_or_else(|_| "English".into()),
                    ),
                };
                let actor = Actor { can_configure };
                (
                    run_language_callback(&self.store, &self.texts().choices(), a, actor, now).await,
                    lang,
                )
            }
        };
        let outcome = match outcome {
            Ok(o) => o,
            Err(e) => {
                log::error!("menu callback {data}: {e}");
                let _ = self.telegram.answer_callback(id, None).await;
                return true;
            }
        };
        match outcome {
            CallbackOutcome::Show { screen, notice } => {
                let toast = notice.map(|n| self.render_message(&lang, &n.message));
                let _ = self.telegram.answer_callback(id, toast.as_deref()).await;
                if let Some(msg) = message {
                    let text = self.render_message(&lang, &screen.message);
                    let _ = self
                        .telegram
                        .edit(msg, &text, self.render_keyboard(&lang, &screen))
                        .await;
                }
            }
            CallbackOutcome::Notify(notice) => {
                let text = self.render_message(&lang, &notice.message);
                let _ = self.telegram.answer_callback(id, Some(&text)).await;
            }
            CallbackOutcome::Ignore => {
                let _ = self.telegram.answer_callback(id, None).await;
            }
        }
        true
    }

    /// `/getlang [file]`: sends that language file, or offers the bases
    /// and then the variants as buttons, each menu led by "All", a zip of
    /// every file it lists (GeneralCommands.cs:470, LanguageHelper.cs:430-500).
    async fn getlang(&self, cmd: &Command, lang: &str) {
        let name = cmd.args.trim().trim_end_matches(".xml");
        if !name.is_empty() {
            if name.contains(['*', '\\', '.', '/']) {
                self.say(cmd.chat.id, "Invalid language file name. Make sure you enter the <b>filename</b> of the file you wish to download.").await;
                return;
            }
            self.send_language_file(cmd.chat.id, name).await;
            return;
        }
        let choices = self.texts().choices();
        let buttons: Vec<Button> = choices
            .bases()
            .into_iter()
            .map(|b| Button::Callback {
                text: b.to_string(),
                data: format!("gl|{}|b|{}", cmd.chat.id, LanguageId::of(b)),
            })
            .collect();
        let mut rows: Keyboard = vec![vec![Button::Callback {
            text: "All".into(),
            data: format!("gl|{}|all|", cmd.chat.id),
        }]];
        rows.extend(buttons.chunks(2).map(|c| c.to_vec()));
        let _ = self
            .telegram
            .send(cmd.chat.id, &self.texts().key(lang, "GetLang", &[]), Some(rows))
            .await;
    }

    async fn getlang_callback(&self, id: &str, _from: &User, rest: &str) {
        let _ = self.telegram.answer_callback(id, None).await;
        let mut parts = rest.splitn(3, '|');
        let (Some(Ok(chat)), Some(step), Some(value)) =
            (parts.next().map(str::parse::<i64>), parts.next(), parts.next())
        else {
            return;
        };
        let choices = self.texts().choices();
        if step == "all" {
            let files: Vec<String> = self.texts().languages().into_iter().map(str::to_string).collect();
            self.send_language_zip(chat, "languages.zip", &files).await;
            return;
        }
        let Ok(id) = value.parse::<LanguageId>() else {
            return;
        };
        match step {
            "b" => {
                let Some(base) = choices.base_by_id(id) else { return };
                match choices.variants(base).as_slice() {
                    [] => {}
                    [only] => self.send_language_file(chat, &only.file.clone()).await,
                    many => {
                        let mut rows: Keyboard = vec![vec![Button::Callback {
                            text: "All".into(),
                            data: format!("gl|{chat}|v|{id}"),
                        }]];
                        rows.extend(many.iter().map(|f| {
                            let text = if f.variant.is_empty() {
                                f.file.clone()
                            } else {
                                f.variant.clone()
                            };
                            vec![Button::Callback {
                                text,
                                data: format!("gl|{chat}|f|{}", LanguageId::of(&f.file)),
                            }]
                        }));
                        let _ = self
                            .telegram
                            .send(chat, &self.texts().key("English", "WhatVariant", &[base]), Some(rows))
                            .await;
                    }
                }
            }
            "v" => {
                let Some(base) = choices.base_by_id(id) else { return };
                let files: Vec<String> = choices.variants(base).iter().map(|f| f.file.clone()).collect();
                let zip_name: String = base
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                    .collect();
                self.send_language_zip(chat, &format!("{zip_name}.zip"), &files).await;
            }
            "f" => {
                if let Some(file) = choices.file_by_id(id).map(|f| f.file.clone()) {
                    self.send_language_file(chat, &file).await;
                }
            }
            _ => {}
        }
    }

    async fn send_language_zip(&self, chat: i64, zip_name: &str, files: &[String]) {
        let texts = self.texts();
        let zipped = zip_files(
            files
                .iter()
                .filter_map(|f| Some((format!("{f}.xml"), texts.file_bytes(f)?))),
        );
        match zipped {
            Ok(bytes) => {
                if let Err(e) = self.telegram.send_document(chat, zip_name, bytes).await {
                    log::warn!("getlang: {e}");
                }
            }
            Err(e) => log::error!("getlang: could not zip language files: {e}"),
        }
    }

    async fn send_language_file(&self, chat: i64, name: &str) {
        match self.texts().file_bytes(name) {
            Some(bytes) => {
                if let Err(e) = self.telegram.send_document(chat, &format!("{name}.xml"), bytes).await {
                    log::warn!("getlang: {e}");
                }
            }
            None => {
                self.say(chat, "No matching language file found. Make sure you enter the <b>filename</b> of the file you wish to download.").await;
            }
        }
    }
}

/// A deflated zip of `(name, contents)` pairs.
fn zip_files(files: impl Iterator<Item = (String, Vec<u8>)>) -> zip::result::ZipResult<Vec<u8>> {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in files {
        zip.start_file(name, options)?;
        zip.write_all(&bytes)?;
    }
    Ok(zip.finish()?.into_inner())
}
