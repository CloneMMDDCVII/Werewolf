//! `/validatelangs` and `/uploadlang` for language admins: the checks are
//! `i18n::admin`'s; this draws its menus and reports, downloads uploads
//! and swaps in the reloaded languages after an install.

use crate::core::BotCore;
use crate::sync::LockExt;
use crate::telegram::{Button, Command, Keyboard, MsgRef, User};
use crate::text::{escape, Texts, ENGLISH_XML};
use i18n::admin::{self, AdminButton, LangFile};
use i18n::{LanguageId, LanguagePack};
use std::path::PathBuf;
use std::sync::Arc;

/// Telegram's limit is 4096 characters; leave room for the markup.
const CHUNK: usize = 3500;

impl BotCore {
    fn is_language_admin(&self, user: i64) -> bool {
        self.config.language_admins.contains(&user) || self.config.global_admins.contains(&user)
    }

    fn installed_languages(&self) -> Vec<LangFile> {
        match admin::load_dir(&self.config.languages_dir) {
            Ok((files, failures)) => {
                for (path, e) in failures {
                    log::warn!("language file {} skipped: {e}", path.display());
                }
                files
            }
            Err(e) => {
                log::warn!("could not read {}: {e}", self.config.languages_dir.display());
                vec![]
            }
        }
    }

    /// AdminCommands.cs:161-218: a menu of bases, two per row.
    pub(crate) async fn validatelangs(&self, cmd: &Command) {
        let files = self.installed_languages();
        let buttons: Vec<Button> = admin::bases(&files)
            .into_iter()
            .map(|b| Button::Callback {
                text: b.to_string(),
                data: AdminButton::PickBase(LanguageId::of(b)).encode(),
            })
            .collect();
        if buttons.is_empty() {
            self.say(cmd.chat.id, "There are no language files to validate.").await;
            return;
        }
        let rows: Keyboard = buttons.chunks(2).map(|c| c.to_vec()).collect();
        let _ = self
            .telegram
            .send(cmd.chat.id, "Validate which language?", Some(rows))
            .await;
    }

    /// LanguageHelper.cs:180-260: check a file sent as a reply, and offer
    /// to install it when nothing fatal was found.
    pub(crate) async fn uploadlang(&self, cmd: &Command) {
        let doc = cmd.replied_document.as_ref();
        if let Err(why) = admin::check_upload_name(doc.and_then(|d| d.file_name.as_deref()).or(doc.map(|_| ""))) {
            self.say(cmd.chat.id, why).await;
            return;
        }
        let doc = doc.expect("checked above");
        let file_name = doc.file_name.clone().unwrap_or_default();
        let bytes = match self.telegram.download(&doc.file_id).await {
            Ok(b) => b,
            Err(e) => {
                self.say(
                    cmd.chat.id,
                    &format!("Could not download the file: {}", escape(&e.to_string())),
                )
                .await;
                return;
            }
        };
        // Kept outside the languages folder until "New" is pressed.
        let dir = std::env::temp_dir().join(format!("werewolf-upload-{}", cmd.from.id));
        let path = dir.join(sanitize(&file_name));
        if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, &bytes)) {
            self.say(
                cmd.chat.id,
                &format!("Could not save the file: {}", escape(&e.to_string())),
            )
            .await;
            return;
        }
        let new = match LangFile::load(&path) {
            Ok(f) => f,
            Err(e) => {
                self.say(
                    cmd.chat.id,
                    &format!("That file isn't a language file I can read: {}", escape(&e.to_string())),
                )
                .await;
                return;
            }
        };
        let check = admin::check_upload(&new, &self.installed_languages(), &master());
        self.send_report(cmd.chat.id, &check.report).await;
        if check.can_upload {
            let id = LanguageId::of(&new.file_name);
            self.uploads.locked().insert(cmd.from.id, (id, path));
            let rows = vec![vec![
                Button::Callback {
                    text: "New".into(),
                    data: AdminButton::KeepNew(id).encode(),
                },
                Button::Callback {
                    text: "Old".into(),
                    data: AdminButton::KeepOld.encode(),
                },
            ]];
            let _ = self.telegram.send(cmd.chat.id, admin::KEEP_WHICH, Some(rows)).await;
        } else {
            self.say(cmd.chat.id, admin::CANNOT_UPLOAD).await;
        }
    }

    /// The `lv|…` and `lu|…` buttons. Returns false for anything else.
    pub(crate) async fn lang_admin_callback(&self, id: &str, from: &User, message: Option<MsgRef>, data: &str) -> bool {
        let Some(button) = AdminButton::decode(data) else {
            return false;
        };
        let _ = self.telegram.answer_callback(id, None).await;
        if !self.is_language_admin(from.id) {
            return true;
        }
        let chat = message.map(|m| m.chat).unwrap_or(from.id);
        match button {
            AdminButton::PickBase(base_id) => {
                let files = self.installed_languages();
                let Some(base) = admin::base_by_id(&files, base_id).map(str::to_string) else {
                    return true;
                };
                let of_base: Vec<&LangFile> = files.iter().filter(|f| f.pack.meta.base == base).collect();
                match of_base.as_slice() {
                    [only] => self.validate_file(chat, only).await,
                    many => {
                        let mut rows: Keyboard = vec![vec![Button::Callback {
                            text: "All".into(),
                            data: AdminButton::ValidateBase(base_id).encode(),
                        }]];
                        rows.extend(many.iter().map(|f| {
                            let text = if f.pack.meta.variant.is_empty() {
                                f.file_name.clone()
                            } else {
                                f.pack.meta.variant.clone()
                            };
                            vec![Button::Callback {
                                text,
                                data: AdminButton::ValidateFile(LanguageId::of(&f.file_name)).encode(),
                            }]
                        }));
                        let text = format!("Validate which {} file?", escape(&base));
                        match message {
                            Some(m) => {
                                let _ = self.telegram.edit(m, &text, Some(rows)).await;
                            }
                            None => {
                                let _ = self.telegram.send(chat, &text, Some(rows)).await;
                            }
                        }
                    }
                }
            }
            AdminButton::ValidateFile(file_id) => {
                let files = self.installed_languages();
                if let Some(file) = admin::file_by_id(&files, file_id) {
                    self.validate_file(chat, file).await;
                }
            }
            AdminButton::ValidateBase(base_id) => {
                let files = self.installed_languages();
                let Some(base) = admin::base_by_id(&files, base_id).map(str::to_string) else {
                    return true;
                };
                let of_base: Vec<LangFile> = files.into_iter().filter(|f| f.pack.meta.base == base).collect();
                let (details, summary) = admin::summary_report(&of_base, &master());
                if !details.is_empty() {
                    self.send_report(chat, &details).await;
                }
                self.send_report(chat, &summary).await;
            }
            AdminButton::KeepNew(file_id) => {
                let pending = self.uploads.locked().remove(&from.id);
                let Some((_, path)) = pending.filter(|(i, _)| *i == file_id) else {
                    self.edit_or_say(message, chat, "That upload has expired. Send /uploadlang again.")
                        .await;
                    return true;
                };
                let result = admin::install(&path, &self.config.languages_dir);
                let _ = std::fs::remove_file(&path);
                let text = match result {
                    Ok(installed) => {
                        if installed.path.is_some() {
                            self.reload_languages();
                        }
                        // The log is fixed text plus file names, with its own <b>.
                        format!(
                            "{}\n{}",
                            installed.log,
                            if installed.path.is_some() {
                                "Languages reloaded."
                            } else {
                                ""
                            }
                        )
                    }
                    Err(e) => format!("Install failed: {}", escape(&e.to_string())),
                };
                self.edit_or_say(message, chat, text.trim()).await;
            }
            AdminButton::KeepOld => {
                if let Some((_, path)) = self.uploads.locked().remove(&from.id) {
                    let _ = std::fs::remove_file(path);
                }
                self.edit_or_say(message, chat, admin::KEPT_OLD).await;
            }
        }
        true
    }

    async fn validate_file(&self, chat: i64, file: &LangFile) {
        let errors = admin::check_file(file, &master());
        self.send_report(chat, &admin::file_report(file, &errors)).await;
    }

    async fn edit_or_say(&self, message: Option<MsgRef>, chat: i64, text: &str) {
        match message {
            Some(m) => {
                let _ = self.telegram.edit(m, text, None).await;
            }
            None => self.say(chat, text).await,
        }
    }

    /// The reports are Markdown in the legacy style; the bot speaks HTML.
    async fn send_report(&self, chat: i64, report: &str) {
        for chunk in chunks(&markdown_to_html(report), CHUNK) {
            self.say(chat, &chunk).await;
        }
    }

    /// Re-reads the languages folder; games already running keep theirs.
    fn reload_languages(&self) {
        let (texts, failures) = Texts::load(&self.config.languages_dir);
        for f in failures {
            log::warn!("language file skipped: {f}");
        }
        self.set_texts(Arc::new(texts));
    }
}

fn master() -> LanguagePack {
    LanguagePack::from_xml(ENGLISH_XML).expect("the embedded English.xml parses")
}

/// A file name safe to write: no folders.
fn sanitize(name: &str) -> PathBuf {
    let base = std::path::Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("upload.xml");
    PathBuf::from(base)
}

/// Escapes `text` and turns paired `*bold*` and `_italic_` on each line
/// into HTML. An unpaired marker stays as it is.
pub fn markdown_to_html(text: &str) -> String {
    text.split('\n')
        .map(|line| convert_line(&escape(line)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn convert_line(line: &str) -> String {
    let mut out = line.to_string();
    for (marker, tag) in [('*', "b"), ('_', "i")] {
        let count = out.matches(marker).count();
        let pairs = count / 2;
        let mut result = String::with_capacity(out.len());
        let mut seen = 0;
        for c in out.chars() {
            if c == marker && seen < pairs * 2 {
                result.push_str(if seen % 2 == 0 { "<" } else { "</" });
                result.push_str(tag);
                result.push('>');
                seen += 1;
            } else {
                result.push(c);
            }
        }
        out = result;
    }
    out
}

/// Splits at line breaks into pieces of at most `max` characters (a
/// longer single line is cut where it must be).
pub fn chunks(text: &str, max: usize) -> Vec<String> {
    let mut out = vec![];
    let mut current = String::new();
    for line in text.split('\n') {
        if !current.is_empty() && current.chars().count() + line.chars().count() + 1 > max {
            out.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
        while current.chars().count() > max {
            let cut: String = current.chars().take(max).collect();
            current = current.chars().skip(max).collect();
            out.push(cut);
        }
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_markdown_becomes_html() {
        assert_eq!(
            markdown_to_html("*Deutsch.xml* (Last updated: 3 May)"),
            "<b>Deutsch.xml</b> (Last updated: 3 May)"
        );
        assert_eq!(
            markdown_to_html("_Missing Values:_\nKey<1>"),
            "<i>Missing Values:</i>\nKey&lt;1&gt;"
        );
        assert_eq!(markdown_to_html("a * b"), "a * b");
    }

    #[test]
    fn long_reports_are_split_at_lines() {
        let text = (0..100).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let parts = chunks(&text, 50);
        assert!(parts.iter().all(|p| p.chars().count() <= 50));
        assert_eq!(parts.join("\n"), text);
    }
}
