//! Loader for the existing `Languages/*.xml` files, unchanged. These are
//! community-translated and hand-edited for years — the format itself is
//! not up for redesign here, only the code that reads it.
//!
//! Shape (see `Werewolf for Telegram/Languages/English.xml`):
//! ```xml
//! <strings>
//!   <language name="English" base="English" variant="Normal" code="en" isDefault="true" />
//!   <string key="PlayerJoined">
//!     <!-- 0 - Player name -->
//!     <value>{0} has joined the game.</value>
//!     <value>{0} joined! Welcome.</value>  <!-- multiple values = picked at random -->
//!   </string>
//! </strings>
//! ```
//!
//! `<value>` content is captured **verbatim**, not as plain text: several
//! community translations embed Telegram HTML formatting directly inside
//! values (e.g. `Some <b>bold</b> text`), which is legal mixed-content XML
//! that the legacy loader (`System.Xml.Linq.XDocument`) handles fine but a
//! naive strict-schema deserializer chokes on. We parse with a manual
//! event reader instead of a rigid struct-derive so that markup, and any
//! other structurally-valid-but-unusual content, round-trips unchanged.

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, Writer};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct LanguageMeta {
    pub name: String,
    pub base: String,
    pub variant: String,
    pub code: Option<String>,
    pub is_default: Option<bool>,
}

/// One loaded language file: its metadata plus key -> variant strings.
/// Deprecated keys are kept out of `strings` but tracked separately, since
/// dropping them silently would make it harder to reason about translation
/// coverage against the still-active key set.
#[derive(Debug)]
pub struct LanguagePack {
    pub meta: LanguageMeta,
    strings: HashMap<String, Vec<String>>,
    pub deprecated_keys: Vec<String>,
    /// Keys that appeared more than once in the source file (translator
    /// error, not a parser error) — first occurrence wins, rest are
    /// recorded here rather than silently dropped or hard-failing the load.
    pub duplicate_keys: Vec<String>,
}

impl LanguagePack {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let raw = fs::read_to_string(&path).map_err(LoadError::Io)?;
        parse(&raw)
    }

    /// All variants for a key, for random-variant selection at call sites.
    pub fn variants(&self, key: &str) -> Option<&[String]> {
        self.strings.get(key).map(|v| v.as_slice())
    }

    /// First (or only) variant for a key — most keys have exactly one.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.strings.get(key)?.first().map(|s| s.as_str())
    }

    /// All value variants across every key, flattened.
    pub fn variants_iter(&self) -> impl Iterator<Item = &str> {
        self.strings.values().flatten().map(|s| s.as_str())
    }
}

fn attr_value(tag: &BytesStart, name: &str) -> Option<String> {
    tag.attributes().flatten().find_map(|a| {
        if a.key.as_ref() == name.as_bytes() {
            Some(String::from_utf8_lossy(&a.value).into_owned())
        } else {
            None
        }
    })
}

fn parse(raw: &str) -> Result<LanguagePack, LoadError> {
    let mut reader = Reader::from_str(raw);
    reader.config_mut().trim_text(true);

    let mut meta = LanguageMeta::default();
    let mut strings: HashMap<String, Vec<String>> = HashMap::new();
    let mut deprecated_keys = vec![];
    let mut duplicate_keys = vec![];

    let mut buf = Vec::new();
    loop {
        let pos_before = reader.buffer_position() as usize;
        let event = reader
            .read_event_into(&mut buf)
            .map_err(LoadError::Xml)?;

        match event {
            Event::Eof => break,
            Event::Empty(ref tag) | Event::Start(ref tag)
                if tag.name().as_ref() == b"language" =>
            {
                meta.name = attr_value(tag, "name").unwrap_or_default();
                meta.base = attr_value(tag, "base").unwrap_or_default();
                meta.variant = attr_value(tag, "variant").unwrap_or_default();
                meta.code = attr_value(tag, "code");
                meta.is_default = attr_value(tag, "isDefault").map(|v| v == "true");
            }
            Event::Start(ref tag) if tag.name().as_ref() == b"string" => {
                let key = attr_value(tag, "key").ok_or(LoadError::MissingKey)?;
                let deprecated = attr_value(tag, "deprecated").map(|v| v == "true");

                let values = read_string_body(&mut reader)?;

                if deprecated == Some(true) {
                    deprecated_keys.push(key);
                } else if strings.contains_key(&key) {
                    duplicate_keys.push(key);
                } else {
                    strings.insert(key, values);
                }
            }
            _ => {
                let _ = pos_before;
            }
        }
        buf.clear();
    }

    Ok(LanguagePack {
        meta,
        strings,
        deprecated_keys,
        duplicate_keys,
    })
}

/// Reads everything inside a `<string>...</string>` element, returning the
/// verbatim inner content of each `<value>` child (markup preserved). Skips
/// comments and anything else that isn't a `<value>`.
fn read_string_body(reader: &mut Reader<&[u8]>) -> Result<Vec<String>, LoadError> {
    let mut values = vec![];
    let mut buf = Vec::new();

    loop {
        let event = reader.read_event_into(&mut buf).map_err(LoadError::Xml)?;
        match event {
            Event::Eof => return Err(LoadError::UnexpectedEof),
            Event::End(ref tag) if tag.name().as_ref() == b"string" => break,
            Event::Start(ref tag) if tag.name().as_ref() == b"value" => {
                let _ = tag;
                values.push(read_raw_inner(reader, b"value")?);
            }
            Event::Empty(ref tag) if tag.name().as_ref() == b"value" => {
                values.push(String::new());
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(values)
}

/// Having just consumed a start tag, replays every event up to its matching
/// close tag back through a `Writer`, reconstructing the inner markup
/// verbatim (nested tags like `<b>...</b>` included) without relying on
/// byte-offset bookkeeping into the original source, which doesn't reliably
/// align once entity decoding/whitespace trimming are in play. Comments are
/// dropped rather than round-tripped.
fn read_raw_inner(reader: &mut Reader<&[u8]>, closing_tag: &[u8]) -> Result<String, LoadError> {
    let mut depth = 0u32;
    let mut writer = Writer::new(Vec::new());
    let mut buf = Vec::new();

    loop {
        let event = reader.read_event_into(&mut buf).map_err(LoadError::Xml)?;
        match event {
            Event::Eof => return Err(LoadError::UnexpectedEof),
            Event::End(ref tag) if depth == 0 && tag.name().as_ref() == closing_tag => break,
            Event::Comment(_) => {}
            Event::Start(_) => {
                depth += 1;
                writer.write_event(event).map_err(LoadError::Xml)?;
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                writer.write_event(event).map_err(LoadError::Xml)?;
            }
            _ => {
                writer.write_event(event).map_err(LoadError::Xml)?;
            }
        }
        buf.clear();
    }

    let bytes = writer.into_inner();
    String::from_utf8(bytes)
        .map(|s| s.trim().to_string())
        .map_err(|_| LoadError::Utf8)
}

#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Xml(quick_xml::Error),
    MissingKey,
    UnexpectedEof,
    Utf8,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "io error: {e}"),
            LoadError::Xml(e) => write!(f, "xml error: {e}"),
            LoadError::MissingKey => write!(f, "<string> element missing key attribute"),
            LoadError::UnexpectedEof => write!(f, "unexpected end of file inside <string>"),
            LoadError::Utf8 => write!(f, "invalid utf-8 in value content"),
        }
    }
}

impl std::error::Error for LoadError {}

/// All loaded language packs, keyed by filename stem (matches how the
/// legacy `Group.Language` column references them).
pub struct Catalog {
    packs: HashMap<String, LanguagePack>,
    /// Files that failed to load, with the reason — surfaced rather than
    /// silently dropped, so a genuinely broken translation file is visible
    /// instead of just quietly missing.
    pub load_failures: Vec<(String, LoadError)>,
}

impl Catalog {
    pub fn load_dir(dir: impl AsRef<Path>) -> Result<Self, LoadError> {
        let mut packs = HashMap::new();
        let mut load_failures = vec![];
        for entry in fs::read_dir(dir).map_err(LoadError::Io)? {
            let entry = entry.map_err(LoadError::Io)?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("xml") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            match LanguagePack::load(&path) {
                Ok(pack) => {
                    packs.insert(stem, pack);
                }
                Err(e) => load_failures.push((stem, e)),
            }
        }
        Ok(Catalog {
            packs,
            load_failures,
        })
    }

    pub fn get(&self, name: &str) -> Option<&LanguagePack> {
        self.packs.get(name)
    }

    pub fn len(&self) -> usize {
        self.packs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packs.is_empty()
    }
}
