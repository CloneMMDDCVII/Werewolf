//! Everything the bot says, looked up in the community language files.
//!
//! English.xml is compiled into the binary, so the bot works with no files
//! on disk; other languages load from the configured folder. A key missing
//! from a group's language falls back to English, as the legacy
//! `GetLocaleString` did. Messages for things the legacy bot never said
//! (or only said through keys now marked deprecated) use English text
//! written here, passed as the `fallback`.
//!
//! Values are Telegram HTML: player names must go through [`escape`], and
//! every `{n}` placeholder is filled or removed, never sent raw.

use game_engine::messages::{render, Message};
use game_engine::roles::PlayerId;
use i18n::LanguagePack;
use rand::seq::SliceRandom;
use shared::Role;
use std::collections::HashMap;
use std::path::Path;

pub const ENGLISH_XML: &str = include_str!("../../../Werewolf for Telegram/Languages/English.xml");

pub struct Texts {
    english: LanguagePack,
    others: HashMap<String, LanguagePack>,
    /// Where each loaded file came from, for `/getlang`.
    paths: HashMap<String, std::path::PathBuf>,
}

impl Texts {
    /// English only, from the copy built into the binary.
    pub fn embedded() -> Self {
        Texts {
            english: LanguagePack::from_xml(ENGLISH_XML).expect("the embedded English.xml parses"),
            others: HashMap::new(),
            paths: HashMap::new(),
        }
    }

    /// The embedded English plus every `*.xml` in `dir`. A missing folder
    /// is fine (English only); files that fail to parse are skipped and
    /// returned so the caller can log them.
    pub fn load(dir: &Path) -> (Self, Vec<String>) {
        let mut texts = Self::embedded();
        let mut failures = vec![];
        let Ok(entries) = std::fs::read_dir(dir) else {
            return (texts, failures);
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("xml") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
                continue;
            };
            if name == "English" {
                continue; // the embedded copy is authoritative for this build
            }
            match LanguagePack::load(&path) {
                Ok(pack) => {
                    texts.paths.insert(name.clone(), path.clone());
                    texts.others.insert(name, pack);
                }
                Err(e) => failures.push(format!("{}: {e}", path.display())),
            }
        }
        (texts, failures)
    }

    pub fn languages(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.others.keys().map(String::as_str).collect();
        v.push("English");
        v.sort_unstable();
        v
    }

    fn variants(&self, lang: &str, key: &str) -> Option<&[String]> {
        self.others
            .get(lang)
            .and_then(|p| p.variants(key))
            .or_else(|| self.english.variants(key))
    }

    /// One variant of `key`'s raw template (picked at random, like
    /// [`Texts::get`]), in `lang` or English.
    pub fn lookup(&self, lang: &str, key: &str) -> Option<String> {
        self.variants(lang, key).and_then(|v| v.choose(&mut rand::thread_rng())).cloned()
    }

    /// An engine message (`game_engine::messages::Message`) as text, with
    /// `name` giving each player's (already escaped) display name.
    /// A role with no description in the language files (the Witch, say)
    /// is described as "You are the Witch." rather than shown as a key.
    pub fn render(&self, lang: &str, message: &Message, name: &dyn Fn(PlayerId) -> String) -> String {
        let lookup = |key: &str| {
            self.lookup(lang, key)
                .or_else(|| key.strip_prefix("RoleInfo").map(|role| format!("You are the {}.", split_camel(role))))
        };
        strip_placeholders(&render(message, &lookup, name)).trim().to_string()
    }

    /// Every loaded language, for the `/setlang` and `/config` pickers.
    pub fn choices(&self) -> control::language_picker::LanguageChoices {
        let file = |name: &str, pack: &LanguagePack| control::language_picker::LanguageFile {
            file: name.to_string(),
            base: if pack.meta.base.is_empty() { name.to_string() } else { pack.meta.base.clone() },
            variant: pack.meta.variant.clone(),
            is_default: pack.meta.is_default == Some(true),
        };
        let mut files: Vec<_> = self.others.iter().map(|(n, p)| file(n, p)).collect();
        files.push(file("English", &self.english));
        control::language_picker::LanguageChoices::new(files)
    }

    /// A language file's bytes, for `/getlang`. English comes from the
    /// copy built into the binary.
    pub fn file_bytes(&self, name: &str) -> Option<Vec<u8>> {
        if name == "English" {
            return Some(ENGLISH_XML.as_bytes().to_vec());
        }
        std::fs::read(self.paths.get(name)?).ok()
    }

    /// The language pack for `lang`, English if there is none by that name.
    pub fn pack(&self, lang: &str) -> &LanguagePack {
        self.others.get(lang).unwrap_or(&self.english)
    }

    /// Whether `key` exists (in `lang` or English).
    pub fn has(&self, lang: &str, key: &str) -> bool {
        self.variants(lang, key).is_some()
    }

    /// The text for `key` with `{0}`, `{1}`, ... replaced by `args` (which
    /// the caller has already HTML-escaped where needed). Picks one variant
    /// at random when a key has several, like the legacy bot. Falls back to
    /// `fallback` (English, same placeholders) when no language has the key.
    pub fn get(&self, lang: &str, key: &str, fallback: &str, args: &[&str]) -> String {
        let template = self
            .variants(lang, key)
            .and_then(|v| v.choose(&mut rand::thread_rng()))
            .map(String::as_str)
            .unwrap_or(fallback);
        fill(template, args)
    }

    /// Like [`Texts::get`] but for keys that must exist in English.xml; a
    /// missing one shows up as the key name in tests rather than as a
    /// silent empty message.
    pub fn key(&self, lang: &str, key: &str, args: &[&str]) -> String {
        self.get(lang, key, key, args)
    }

    /// A role's display name ("Seer", "Wolf"...), localised where the
    /// language has a key for it.
    pub fn role_name(&self, lang: &str, role: Role) -> String {
        let key = match role {
            Role::Doppelganger => "Doppelgänger".to_string(),
            _ => role_key(role),
        };
        self.get(lang, &key, &split_camel(&role_key(role)), &[])
    }
}

/// `Role`'s serde name, which is also the legacy enum name the locale keys
/// are built from (`RoleInfoSeer`, `RoleInfoGuardianAngel`).
pub fn role_key(role: Role) -> String {
    format!("{role:?}")
}

fn split_camel(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && c.is_uppercase() {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// Replaces `{0}`, `{1}`, ... with `args`, removes any placeholder left
/// without an argument, and turns the files' literal `\n` into newlines.
pub fn fill(template: &str, args: &[&str]) -> String {
    let mut out = template.replace("\\n", "\n");
    for (i, a) in args.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), a);
    }
    strip_placeholders(&out).trim().to_string()
}

fn strip_placeholders(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let digits = after.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && after[digits..].starts_with('}') {
            out.push_str(&rest[..start]);
            rest = &after[digits + 1..];
        } else {
            out.push_str(&rest[..=start]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Escapes text for Telegram's HTML parse mode. Every player and group
/// name goes through this before it's put into a message.
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Bold, the legacy `ToBold()`.
pub fn bold(s: &str) -> String {
    format!("<b>{s}</b>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use game_engine::role_info::role_info;

    #[test]
    fn embedded_english_loads_and_fills() {
        let t = Texts::embedded();
        assert_eq!(t.key("English", "SecondsLeftToJoin", &["30"]), "30 seconds left to Join");
        assert_eq!(t.key("Klingon", "SecondsLeftToJoin", &["30"]), "30 seconds left to Join");
    }

    #[test]
    fn literal_newlines_and_unfilled_placeholders() {
        assert_eq!(fill("a\\nb {0} {1}", &["x"]), "a\nb x");
        assert_eq!(fill("{0}{0}", &["y"]), "yy");
        assert_eq!(fill("{not a placeholder}", &[]), "{not a placeholder}");
    }

    #[test]
    fn missing_keys_use_the_fallback() {
        let t = Texts::embedded();
        assert_eq!(t.get("English", "NoSuchKey", "Hello {0}", &["Ana"]), "Hello Ana");
    }

    #[test]
    fn role_names_and_infos() {
        let t = Texts::embedded();
        assert_eq!(t.role_name("English", Role::Seer), "the Seer 👳");
        let info = |role| t.render("English", &role_info(role, &[], false), &|_| String::new());
        assert!(info(Role::Seer).contains("seer"));
        assert_eq!(info(Role::Witch), "You are the Witch.");
    }

    #[test]
    fn every_shipped_language_file_loads() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Werewolf for Telegram/Languages");
        let (texts, failures) = Texts::load(&dir);
        assert!(failures.is_empty(), "{failures:#?}");
        assert!(texts.languages().len() > 100);
    }

    #[test]
    fn names_are_escaped() {
        assert_eq!(escape("<b>Ana</b> & co"), "&lt;b&gt;Ana&lt;/b&gt; &amp; co");
    }
}
