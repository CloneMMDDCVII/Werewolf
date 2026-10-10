//! The language admins' tools, `/validatelangs` and `/uploadlang`
//! (Werewolf Control `Helpers/LanguageHelper.cs`): checking a language
//! file against English.xml, comparing an uploaded file with the one it
//! would replace, and installing it.
//!
//! Pure over files on disk. Downloading the upload from Telegram, the
//! base/variant menus and the New/Old buttons are the presenter's. Not
//! ported: committing English.xml to git from the bot, the `Language` DB
//! table, and broadcasting reloads to nodes (one process now: reload the
//! `Catalog` after `install`).

use crate::{LanguageId, LanguagePack, LoadError};
use shared::time::Timestamp;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The file every other language is checked against (`Program.MasterLanguage`).
pub const MASTER_FILE: &str = "English.xml";

/// A language file and what the tools need to know about it (`LangFile`,
/// LanguageHelper.cs:23-47).
#[derive(Debug)]
pub struct LangFile {
    /// File name without `.xml`; the name groups and players refer to.
    pub file_name: String,
    pub path: PathBuf,
    pub pack: LanguagePack,
    pub updated: Timestamp,
}

impl LangFile {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let path = path.as_ref();
        let pack = LanguagePack::load(path)?;
        let modified = fs::metadata(path).and_then(|m| m.modified()).map_err(LoadError::Io)?;
        Ok(LangFile {
            file_name: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string(),
            path: path.to_path_buf(),
            pack,
            updated: Timestamp::from_system_time(modified),
        })
    }

    fn is_default(&self) -> bool {
        self.pack.meta.is_default.unwrap_or(false)
    }
}

/// Files in a directory that didn't parse, with why.
pub type LoadFailures = Vec<(PathBuf, LoadError)>;

/// Every `*.xml` in `dir`, sorted by file name. Files that don't parse are
/// returned separately rather than aborting the listing.
pub fn load_dir(dir: impl AsRef<Path>) -> io::Result<(Vec<LangFile>, LoadFailures)> {
    let mut files = vec![];
    let mut failures = vec![];
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("xml") {
            continue;
        }
        match LangFile::load(&path) {
            Ok(file) => files.push(file),
            Err(e) => failures.push((path, e)),
        }
    }
    files.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok((files, failures))
}

/// The bases a `/validatelangs` menu offers: each once, sorted.
pub fn bases(files: &[LangFile]) -> Vec<&str> {
    let mut bases: Vec<&str> = files.iter().map(|f| f.pack.meta.base.as_str()).collect();
    bases.sort_unstable();
    bases.dedup();
    bases
}

/// The file with this id among `files`.
pub fn file_by_id(files: &[LangFile], id: LanguageId) -> Option<&LangFile> {
    files.iter().find(|f| LanguageId::of(&f.file_name) == id)
}

/// The base language with this id among `files`.
pub fn base_by_id(files: &[LangFile], id: LanguageId) -> Option<&str> {
    id.find_in(bases(files))
}

/// A button in the language admins' menus, as carried in Telegram callback
/// data (the legacy `validate|…` and `upload|…` callbacks,
/// UpdateHandler.cs:1263-1298). Languages are named by [`LanguageId`], so
/// every button fits Telegram's 64 bytes whatever the names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminButton {
    /// A base in the `/validatelangs` menu: validate it if it has one
    /// file, else offer its variants.
    PickBase(LanguageId),
    /// "All" in the variant menu: validate every file of the base.
    ValidateBase(LanguageId),
    /// One variant: validate that file.
    ValidateFile(LanguageId),
    /// "New" after `/uploadlang`: install the uploaded file with this id
    /// (the presenter finds it among its pending uploads).
    KeepNew(LanguageId),
    /// "Old": [`KEPT_OLD`].
    KeepOld,
}

impl AdminButton {
    pub fn encode(&self) -> String {
        match self {
            AdminButton::PickBase(id) => format!("lv|b|{id}"),
            AdminButton::ValidateBase(id) => format!("lv|a|{id}"),
            AdminButton::ValidateFile(id) => format!("lv|f|{id}"),
            AdminButton::KeepNew(id) => format!("lu|n|{id}"),
            AdminButton::KeepOld => "lu|o".to_string(),
        }
    }

    pub fn decode(data: &str) -> Option<Self> {
        let mut parts = data.split('|');
        let button = match (parts.next()?, parts.next()?, parts.next()) {
            ("lv", "b", Some(id)) => AdminButton::PickBase(id.parse().ok()?),
            ("lv", "a", Some(id)) => AdminButton::ValidateBase(id.parse().ok()?),
            ("lv", "f", Some(id)) => AdminButton::ValidateFile(id.parse().ok()?),
            ("lu", "n", Some(id)) => AdminButton::KeepNew(id.parse().ok()?),
            ("lu", "o", None) => AdminButton::KeepOld,
            _ => return None,
        };
        parts.next().is_none().then_some(button)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ErrorLevel {
    DuplicatedString,
    MissingString,
    Error,
    FatalError,
    JoinLink,
    Ads,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageError {
    pub file: String,
    pub key: String,
    pub message: String,
    pub level: ErrorLevel,
}

impl LanguageError {
    fn new(file: &str, key: &str, message: impl Into<String>, level: ErrorLevel) -> Self {
        LanguageError {
            file: file.into(),
            key: key.into(),
            message: message.into(),
            level,
        }
    }
}

const NODE: &str = "*Language Node*";

/// `CheckLanguageNode` (LanguageHelper.cs:509-518).
fn check_language_node(file: &LangFile, errors: &mut Vec<LanguageError>) {
    let meta = &file.pack.meta;
    for (value, message) in [
        (&meta.name, "Language name is missing"),
        (&meta.base, "Base is missing"),
        (&meta.variant, "Variant is missing"),
    ] {
        if value.trim().is_empty() {
            errors.push(LanguageError::new(
                &file.file_name,
                NODE,
                message,
                ErrorLevel::FatalError,
            ));
        }
    }
}

/// `GetFileErrors` (LanguageHelper.cs:602-665): duplicates, missing
/// strings, placeholder mismatches, oversized GIF captions, `/join`, ads.
fn check_strings(file: &LangFile, master: &LanguagePack, errors: &mut Vec<LanguageError>) {
    let name = file.file_name.as_str();
    // The C# only looked for CultConvertSerialKiller and CupidChosen; any
    // duplicate has the same effect (the second one is never used).
    for key in &file.pack.duplicate_keys {
        errors.push(LanguageError::new(
            name,
            key,
            format!("{key} duplication"),
            ErrorLevel::DuplicatedString,
        ));
    }

    let mut keys: Vec<&str> = master.keys().collect();
    keys.sort_unstable();
    for key in keys {
        let Some(values) = file.pack.variants(key) else {
            errors.push(LanguageError::new(
                name,
                key,
                "Values missing",
                ErrorLevel::MissingString,
            ));
            continue;
        };
        let english = master.get(key).unwrap_or_default();
        let placeholders = (0..5)
            .filter(|i| english.contains(&format!("{{{i}}}")))
            .map(|i| i + 1)
            .max()
            .unwrap_or(0);
        let is_gif = master.gif_keys.iter().any(|k| k == key);

        for value in values {
            for i in 0..5 {
                let has = value.contains(&format!("{{{i}}}"));
                if !has && i < placeholders {
                    errors.push(LanguageError::new(
                        name,
                        key,
                        format!("Missing {{{i}}}"),
                        ErrorLevel::Error,
                    ));
                } else if has && i >= placeholders {
                    errors.push(LanguageError::new(
                        name,
                        key,
                        format!("Extra {{{i}}}"),
                        ErrorLevel::Error,
                    ));
                }
            }
            if is_gif && value.chars().count() > 1000 {
                errors.push(LanguageError::new(
                    name,
                    key,
                    "GIF string length cannot exceed 1000 characters",
                    ErrorLevel::FatalError,
                ));
            }
            let join_flagged = errors.iter().any(|e| e.key == key && e.level == ErrorLevel::JoinLink);
            if value.to_lowercase().contains("/join") && !join_flagged {
                errors.push(LanguageError::new(name, key, "", ErrorLevel::JoinLink));
            }
            if contains_ad(value) {
                errors.push(LanguageError::new(
                    name,
                    key,
                    "No usernames or links in the langfile!",
                    ErrorLevel::Ads,
                ));
            }
        }
    }
}

/// Every check `/validatelangs` runs on one file, in the C#'s order.
///
/// Not ported: `TestLength` (LanguageHelper.cs:589-600), which refused
/// files whose base, variant or file name would push the legacy button
/// data past Telegram's 64 bytes. Buttons carry a [`LanguageId`] now, so
/// names have no length limit.
pub fn check_file(file: &LangFile, master: &LanguagePack) -> Vec<LanguageError> {
    let mut errors = vec![];
    check_language_node(file, &mut errors);
    check_strings(file, master, &mut errors);
    errors
}

/// Accounts the bot itself runs; mentioning them isn't advertising.
const HOUSE_ACCOUNTS: [&str; 6] = [
    "werewolfbot",
    "werewolfbetabot",
    "greywolfdev",
    "werewolfsupport",
    "greywolfsupport",
    "para949",
];

/// The C#'s ad pattern (LanguageHelper.cs:659), case-insensitive: a group
/// invite link (`t.me/+…`, `telegram.me/joinchat/…`), or ` @name` for any
/// name not starting with a house account.
pub fn contains_ad(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let invite = ["t.me/", "telegram.me/"].iter().any(|host| {
        lower.match_indices(host).any(|(at, _)| {
            let rest = &lower[at + host.len()..];
            let code = rest.strip_prefix('+').or_else(|| rest.strip_prefix("joinchat/"));
            code.and_then(|c| c.chars().next())
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
    });
    let mention = lower.match_indices(" @").any(|(at, _)| {
        let rest = &lower[at + 2..];
        rest.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_')
            && !HOUSE_ACCOUNTS.iter().any(|house| rest.starts_with(house))
    });
    invite || mention
}

fn of_level(errors: &[LanguageError], level: ErrorLevel) -> impl Iterator<Item = &LanguageError> {
    errors.iter().filter(move |e| e.level == level)
}

fn count(errors: &[LanguageError], level: ErrorLevel) -> usize {
    of_level(errors, level).count()
}

fn keys_joined(errors: &[LanguageError], level: ErrorLevel) -> String {
    of_level(errors, level)
        .map(|e| e.key.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// `/validatelangs` on one file (`ValidateLanguageFile`,
/// LanguageHelper.cs:120-178). Telegram Markdown.
pub fn file_report(file: &LangFile, errors: &[LanguageError]) -> String {
    let mut out = format!(
        "*{}.xml* (Last updated: {})\n",
        file.file_name,
        file.updated.month_day()
    );
    if count(errors, ErrorLevel::Ads) > 0 {
        out += "*ADS DETECTED*\n";
        for e in of_level(errors, ErrorLevel::Ads) {
            out += &format!("{}\n", e.key);
        }
    } else {
        if count(errors, ErrorLevel::Error) > 0 {
            out += "_Errors:_\n";
            for e in of_level(errors, ErrorLevel::Error) {
                out += &format!("{}\n{}\n\n", e.key, e.message);
            }
        }
        if count(errors, ErrorLevel::MissingString) > 0 {
            out += "_Missing Values:_\n";
            for e in of_level(errors, ErrorLevel::MissingString) {
                out += &format!("{}\n", e.key);
            }
        }
        if count(errors, ErrorLevel::DuplicatedString) > 0 {
            out += &format!(
                "\n_Duplicated Strings:_\n{}\n",
                keys_joined(errors, ErrorLevel::DuplicatedString)
            );
        }
        if count(errors, ErrorLevel::JoinLink) > 0 {
            out += &format!("\n_Join commands:_\n{}\n", keys_joined(errors, ErrorLevel::JoinLink));
        }
        if count(errors, ErrorLevel::FatalError) > 0 {
            out += "\n*Fatal errors:*\n";
            for e in of_level(errors, ErrorLevel::FatalError) {
                out += &format!("{}\n{}\n", e.key, e.message);
            }
        }
    }
    out += &format!(
        "\n*Validation complete*.\nErrors: {}\nMissing strings: {}",
        count(errors, ErrorLevel::Error),
        count(errors, ErrorLevel::MissingString)
    );
    out
}

/// `/validatelangs` over several files (`ValidateFiles`,
/// LanguageHelper.cs:64-118): a section per file with problems, then the
/// summary. `files` is every file of the chosen base (or all files); the
/// master file is skipped for checks but counts toward "most/least
/// recently updated", as in the C#.
///
/// Changes: ads are checked per file (the C# checked the whole run, so
/// one file with ads hid every later file's details), headers only appear
/// for files that have that problem, and fatal errors are listed too.
pub fn summary_report(files: &[LangFile], master: &LanguagePack) -> (String, String) {
    let mut details = String::new();
    let mut all_errors = vec![];
    for file in files
        .iter()
        .filter(|f| f.file_name != MASTER_FILE.trim_end_matches(".xml"))
    {
        let errors = check_file(file, master);
        if errors.is_empty() {
            continue;
        }
        details += &format!(
            "*{}.xml* (Last updated: {})\n",
            file.file_name,
            file.updated.month_day()
        );
        if count(&errors, ErrorLevel::Ads) > 0 {
            details += &format!("*Ads detected:* {}\n", keys_joined(&errors, ErrorLevel::Ads));
        } else {
            if count(&errors, ErrorLevel::DuplicatedString) > 0 {
                details += &format!(
                    "_Duplicated Strings: _{}\n",
                    keys_joined(&errors, ErrorLevel::DuplicatedString)
                );
            }
            if count(&errors, ErrorLevel::JoinLink) > 0 {
                details += &format!("_Join commands: _{}\n", keys_joined(&errors, ErrorLevel::JoinLink));
            }
            details += &format!("_Missing strings:_ {}\n", count(&errors, ErrorLevel::MissingString));
            for e in errors
                .iter()
                .filter(|e| matches!(e.level, ErrorLevel::Error | ErrorLevel::FatalError))
            {
                details += &format!("_{:?} - {}_\n{}\n", e.level, e.key, e.message);
            }
            details += "\n";
        }
        all_errors.extend(errors);
    }

    let mut summary = format!(
        "*Validation complete*\nErrors: {}\nMissing strings: {}",
        count(&all_errors, ErrorLevel::Error),
        count(&all_errors, ErrorLevel::MissingString)
    );
    let newest = files.iter().max_by_key(|f| f.updated);
    let oldest = files.iter().min_by_key(|f| f.updated);
    if let (Some(newest), Some(oldest)) = (newest, oldest) {
        summary += &format!(
            "\nMost recently updated file: {}.xml ({})\nLeast recently updated file: {}.xml ({})",
            newest.file_name,
            newest.updated.month_day(),
            oldest.file_name,
            oldest.updated.month_day()
        );
    }
    (details, summary)
}

pub const MUST_REPLY: &str = "Please reply to the file with /uploadlang";
pub const MUST_BE_XML: &str = "The file must be an XML file! (*.xml)";
pub const KEEP_WHICH: &str = "Which file do you want to keep?";
pub const CANNOT_UPLOAD: &str = "Fatal errors present, cannot upload.";
pub const KEPT_OLD: &str = "No action taken.";

/// `/uploadlang`'s first checks (AdminCommands.cs:102-129): the command
/// must reply to a document, and that document must be `.xml`.
pub fn check_upload_name(replied_document: Option<&str>) -> Result<(), &'static str> {
    match replied_document {
        None => Err(MUST_REPLY),
        Some(name) if !name.to_lowercase().ends_with(".xml") => Err(MUST_BE_XML),
        Some(_) => Ok(()),
    }
}

/// An installed file the upload partly matches (LanguageHelper.cs:206-219):
/// same file name but another language, same language but another base or
/// variant, or same base and variant under another file name.
fn upload_conflict<'a>(new: &LangFile, installed: &'a [LangFile]) -> Option<&'a LangFile> {
    let n = &new.pack.meta;
    installed.iter().find(|x| {
        let m = &x.pack.meta;
        (x.file_name.to_lowercase() == new.file_name.to_lowercase() && m.name != n.name)
            || (m.name == n.name && (m.base != n.base || m.variant != n.variant))
            || (m.base == n.base && m.variant == n.variant && x.file_name != new.file_name)
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadCheck {
    /// The comparison to show the admin (Telegram Markdown).
    pub report: String,
    pub errors: Vec<LanguageError>,
    /// No fatal errors and no ads: offer "New" / "Old" ([`KEEP_WHICH`]);
    /// otherwise say [`CANNOT_UPLOAD`].
    pub can_upload: bool,
}

/// `/uploadlang` after the download (`UploadFile` and `OutputResult`,
/// LanguageHelper.cs:180-269 and 520-587): checks the new file, finds the
/// installed file with the same language name, and compares them.
pub fn check_upload(new: &LangFile, installed: &[LangFile], master: &LanguagePack) -> UploadCheck {
    let mut errors = vec![];
    check_language_node(new, &mut errors);
    if let Some(clash) = upload_conflict(new, installed) {
        let m = &clash.pack.meta;
        errors.push(LanguageError::new(
            &new.file_name,
            NODE,
            format!(
                "ERROR: The following file partially matches the same language node. Please check the file name, \
                 and the language name, base and variant. Aborting.\n\n*{}.xml*\n_Name:_{}\n_Base:_{}\n_Variant:_{}",
                clash.file_name, m.name, m.base, m.variant
            ),
            ErrorLevel::FatalError,
        ));
    }
    check_strings(new, master, &mut errors);

    let current = installed.iter().find(|x| x.pack.meta.name == new.pack.meta.name);
    let mut report = format!("NEW FILE\n*{}.xml - ({})*\n", new.file_name, new.pack.meta.name);
    if count(&errors, ErrorLevel::Ads) > 0 {
        report += "*ADS DETECTED*\n";
        for e in of_level(&errors, ErrorLevel::Ads) {
            report += &format!("{}\n", e.key);
        }
    } else {
        if count(&errors, ErrorLevel::Error) > 0 {
            report += "_Errors:_\n";
            for e in of_level(&errors, ErrorLevel::Error) {
                report += &format!("{}\n{}\n\n", e.key, e.message);
            }
        }
        if count(&errors, ErrorLevel::MissingString) > 0 {
            report += "_Missing Values:_\n";
            for e in of_level(&errors, ErrorLevel::MissingString) {
                report += &format!("{}\n", e.key);
            }
        }
        if count(&errors, ErrorLevel::DuplicatedString) > 0 {
            report += "\n_Warning:_\n";
            for e in of_level(&errors, ErrorLevel::DuplicatedString) {
                report += &format!("{}\n", e.message);
            }
            report += "The second instance of the string won't be used, unless you move one of the two values \
                       inside the other. Check the latest English file to see how this is fixed.\n\n";
        }
        if count(&errors, ErrorLevel::JoinLink) > 0 {
            report += "\n_Join commands detected:_\n";
            for e in of_level(&errors, ErrorLevel::JoinLink) {
                report += &format!("{}\n", e.key);
            }
            report +=
                "These strings won't be used, and will fall back to English, until you remove `/join` from them.\n\n";
        }
        if count(&errors, ErrorLevel::FatalError) > 0 {
            report += "\n*Fatal errors:*\n";
            for e in of_level(&errors, ErrorLevel::FatalError) {
                report += &format!("{}\n{}\n\n", e.key, e.message);
            }
        }
        if errors.is_empty() {
            report += "_No errors_\n";
        }
    }

    match current {
        Some(cur) => {
            let mut cur_errors = vec![];
            check_strings(cur, master, &mut cur_errors);
            report += &format!(
                "\n\nOLD FILE (Last updated: {})\n*{}.xml - ({})*\nErrors: {}\nMissing strings: {}",
                cur.updated.month_day(),
                cur.file_name,
                cur.pack.meta.name,
                count(&cur_errors, ErrorLevel::Error),
                count(&cur_errors, ErrorLevel::MissingString)
            );
            if new.is_default() != cur.is_default() {
                report += &format!(
                    "\n\n*isDefault changed from {} to {}*",
                    title_bool(cur.is_default()),
                    title_bool(new.is_default())
                );
            }
        }
        None => {
            let meta = &new.pack.meta;
            report += "\n\n*No old file, this is a new language*";
            report += "\nPlease double check the filename, and the language name, base and variant, as you won't be able to change them.";
            report += &format!("\n_Name:_ {}", meta.name);
            report += &format!("\n_Base:_ {}", meta.base);
            if !installed.iter().any(|x| x.pack.meta.base == meta.base) {
                report += " *(NEW)*";
            }
            report += &format!("\n_Variant:_ {}", meta.variant);
            if new.is_default() {
                report += "\n*isDefault: true*";
            }
        }
    }

    let can_upload = !errors
        .iter()
        .any(|e| matches!(e.level, ErrorLevel::FatalError | ErrorLevel::Ads));
    UploadCheck {
        report,
        errors,
        can_upload,
    }
}

/// C# prints booleans as `True`/`False`.
fn title_bool(b: bool) -> &'static str {
    if b {
        "True"
    } else {
        "False"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// The progress log to show the admin (HTML).
    pub log: String,
    /// Where the file now lives, or `None` if the install was refused.
    pub path: Option<PathBuf>,
}

/// The "New" button (`UseNewLanguageFile`, LanguageHelper.cs:271-430):
/// moves the uploaded file into `language_dir`, replacing the installed
/// file with the same language name (even under another file name). It
/// refuses when a different language already has the same base and
/// variant. The caller reloads its `Catalog` afterwards.
pub fn install(uploaded: &Path, language_dir: &Path) -> Result<Installed, LoadError> {
    let mut log = String::from("Moving file to production..\nChecking paths for duplicate language file...\n");
    let new = LangFile::load(uploaded)?;
    let file_name = uploaded.file_name().unwrap_or_default();
    let mut target = language_dir.join(file_name);
    let (installed, _) = load_dir(language_dir).map_err(LoadError::Io)?;
    let meta = &new.pack.meta;

    if let Some(same_name) = installed
        .iter()
        .find(|x| x.pack.meta.name == meta.name && x.path != target)
    {
        log += &format!(
            "Found duplicate language (name attribute) with filename {}\n",
            same_name.file_name
        );
        target = same_name.path.clone();
    } else if let Some(clash) = installed
        .iter()
        .find(|x| x.pack.meta.base == meta.base && x.pack.meta.variant == meta.variant && x.pack.meta.name != meta.name)
    {
        log += &format!(
            "Found duplicate language (matching base and variant) with filename {}\nAborting!",
            clash.file_name
        );
        return Ok(Installed { log, path: None });
    }

    fs::copy(uploaded, &target).map_err(LoadError::Io)?;
    fs::remove_file(uploaded).map_err(LoadError::Io)?;
    log += "File copied to bot\n\n<b>Operation complete.</b>";
    Ok(Installed {
        log,
        path: Some(target),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_str;

    pub(crate) fn lang(file_name: &str, name: &str, base: &str, variant: &str, body: &str) -> LangFile {
        let xml = format!(
            r#"<strings><language name="{name}" base="{base}" variant="{variant}" isDefault="false" />{body}</strings>"#
        );
        LangFile {
            file_name: file_name.into(),
            path: PathBuf::from(format!("{file_name}.xml")),
            pack: parse_str(&xml).unwrap(),
            updated: Timestamp::from_utc(2026, 10, 3, 0, 0, 0),
        }
    }

    fn master() -> LanguagePack {
        parse_str(
            r#"<strings><language name="English" base="English" variant="Normal" />
            <string key="Joined"><value>{0} joined.</value></string>
            <string key="Killed"><value>{0} killed {1}.</value></string>
            <string key="Start" isgif="true"><value>Game on!</value></string>
            <string key="Old" deprecated="true"><value>gone</value></string>
            </strings>"#,
        )
        .unwrap()
    }

    const COMPLETE: &str = r#"<string key="Joined"><value>{0} ist da.</value></string>
        <string key="Killed"><value>{0} tötet {1}.</value></string>
        <string key="Start"><value>Los!</value></string>"#;

    fn levels(errors: &[LanguageError]) -> Vec<(String, ErrorLevel)> {
        errors.iter().map(|e| (e.key.clone(), e.level)).collect()
    }

    #[test]
    fn a_complete_file_has_no_errors() {
        let file = lang("German", "Deutsch", "German", "Normal", COMPLETE);
        assert_eq!(check_file(&file, &master()), vec![]);
    }

    #[test]
    fn missing_node_attributes_are_fatal() {
        let file = lang("German", "Deutsch", "German", "", COMPLETE);
        let errors = check_file(&file, &master());
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "Variant is missing");
        assert_eq!(errors[0].key, "*Language Node*");
        assert_eq!(errors[0].level, ErrorLevel::FatalError);
    }

    #[test]
    fn long_names_are_fine_now_that_buttons_carry_ids() {
        let file = lang(&"F".repeat(60), "Deutsch", &"B".repeat(40), &"V".repeat(40), COMPLETE);
        assert_eq!(check_file(&file, &master()), vec![]);
    }

    #[test]
    fn admin_buttons_round_trip_and_stay_short() {
        let long = LanguageId::of(&"Ü".repeat(200));
        for button in [
            AdminButton::PickBase(long),
            AdminButton::ValidateBase(long),
            AdminButton::ValidateFile(long),
            AdminButton::KeepNew(long),
            AdminButton::KeepOld,
        ] {
            let data = button.encode();
            assert!(data.len() <= 64, "{data}");
            assert_eq!(AdminButton::decode(&data), Some(button));
        }
        assert_eq!(AdminButton::decode("lv|f|nothex"), None);
        assert_eq!(AdminButton::decode("lu|o|extra"), None);
        assert_eq!(AdminButton::decode("validate|1|English|null|base"), None);
    }

    #[test]
    fn ids_resolve_to_files_and_bases() {
        let files = [
            lang("German", "Deutsch", "German", "Normal", ""),
            lang("Pirate", "Arr", "English", "Pirate", ""),
        ];
        assert_eq!(
            file_by_id(&files, LanguageId::of("Pirate")).map(|f| f.pack.meta.name.as_str()),
            Some("Arr")
        );
        assert_eq!(base_by_id(&files, LanguageId::of("English")), Some("English"));
        assert_eq!(file_by_id(&files, LanguageId::of("Gone")).map(|f| &f.file_name), None);
    }

    #[test]
    fn missing_strings_skip_deprecated_ones() {
        let file = lang(
            "German",
            "Deutsch",
            "German",
            "Normal",
            r#"<string key="Joined"><value>{0}</value></string>"#,
        );
        assert_eq!(
            levels(&check_file(&file, &master())),
            vec![
                ("Killed".into(), ErrorLevel::MissingString),
                ("Start".into(), ErrorLevel::MissingString)
            ]
        );
    }

    #[test]
    fn placeholders_must_match_english() {
        let body = r#"<string key="Joined"><value>ist da {1}</value></string>
            <string key="Killed"><value>{0} tötet {1}.</value><value>{1} stirbt</value></string>
            <string key="Start"><value>Los!</value></string>"#;
        let errors = check_file(&lang("German", "Deutsch", "German", "Normal", body), &master());
        let messages: Vec<_> = errors.iter().map(|e| format!("{} {}", e.key, e.message)).collect();
        assert_eq!(
            messages,
            vec!["Joined Missing {0}", "Joined Extra {1}", "Killed Missing {0}"]
        );
        assert!(errors.iter().all(|e| e.level == ErrorLevel::Error));
    }

    #[test]
    fn long_gif_captions_are_fatal() {
        let body = COMPLETE.replace("Los!", &"é".repeat(1001));
        let errors = check_file(&lang("German", "Deutsch", "German", "Normal", &body), &master());
        assert_eq!(levels(&errors), vec![("Start".into(), ErrorLevel::FatalError)]);
        let body = COMPLETE.replace("Los!", &"é".repeat(1000));
        assert!(check_file(&lang("German", "Deutsch", "German", "Normal", &body), &master()).is_empty());
    }

    #[test]
    fn join_is_flagged_once_per_key() {
        let body = COMPLETE.replace(
            "<value>Los!</value>",
            "<value>Tippe /JOIN</value><value>/join jetzt</value>",
        );
        let errors = check_file(&lang("German", "Deutsch", "German", "Normal", &body), &master());
        assert_eq!(levels(&errors), vec![("Start".into(), ErrorLevel::JoinLink)]);
    }

    #[test]
    fn duplicated_keys_are_warnings() {
        let body = format!(r#"{COMPLETE}<string key="Start"><value>Nochmal</value></string>"#);
        let errors = check_file(&lang("German", "Deutsch", "German", "Normal", &body), &master());
        assert_eq!(levels(&errors), vec![("Start".into(), ErrorLevel::DuplicatedString)]);
        assert_eq!(errors[0].message, "Start duplication");
    }

    #[test]
    fn ads_are_invite_links_and_foreign_mentions() {
        assert!(contains_ad("join t.me/+AbC_1"));
        assert!(contains_ad("https://telegram.me/joinchat/xyz"));
        assert!(contains_ad("T.ME/+x"));
        assert!(contains_ad("ask @someone"));
        assert!(!contains_ad("ask @werewolfsupport or @Para949"));
        assert!(!contains_ad("play with @WerewolfBot"));
        assert!(!contains_ad("t.me/werewolfsupport"));
        assert!(!contains_ad("mail me@example.com"));
        assert!(!contains_ad("just @ alone"));
        assert!(
            !contains_ad("@start of a value"),
            "the C# pattern needs a space before @"
        );
    }

    #[test]
    fn file_report_lists_problems_by_kind() {
        let body = r#"<string key="Joined"><value>ist da</value></string>"#;
        let file = lang("German", "Deutsch", "German", "Normal", body);
        let report = file_report(&file, &check_file(&file, &master()));
        assert_eq!(
            report,
            "*German.xml* (Last updated: Oct 03)\n_Errors:_\nJoined\nMissing {0}\n\n\
             _Missing Values:_\nKilled\nStart\n\n*Validation complete*.\nErrors: 1\nMissing strings: 2"
        );
    }

    #[test]
    fn file_report_shows_only_ads_when_there_are_any() {
        let body = COMPLETE.replace("Los!", "Los! t.me/+abc");
        let file = lang("German", "Deutsch", "German", "Normal", &body);
        let report = file_report(&file, &check_file(&file, &master()));
        assert!(report.starts_with("*German.xml* (Last updated: Oct 03)\n*ADS DETECTED*\nStart\n"));
    }

    #[test]
    fn summary_reports_each_file_with_problems() {
        let mut clean = lang("German", "Deutsch", "German", "Normal", COMPLETE);
        clean.updated = Timestamp::from_utc(2026, 1, 5, 0, 0, 0);
        let ads = lang(
            "GermanAds",
            "Werbung",
            "German",
            "Ads",
            &COMPLETE.replace("Los!", "Los @spam"),
        );
        let missing = lang(
            "GermanShort",
            "Kurz",
            "German",
            "Short",
            r#"<string key="Joined"><value>da</value></string>"#,
        );
        let (details, summary) = summary_report(&[clean, ads, missing], &master());
        assert_eq!(
            details,
            "*GermanAds.xml* (Last updated: Oct 03)\n*Ads detected:* Start\n\
             *GermanShort.xml* (Last updated: Oct 03)\n_Missing strings:_ 2\n_Error - Joined_\nMissing {0}\n\n"
        );
        assert!(summary.starts_with("*Validation complete*\nErrors: 1\nMissing strings: 2\n"));
        assert!(summary.ends_with("Least recently updated file: German.xml (Jan 05)"));
    }

    #[test]
    fn bases_are_listed_once_and_sorted() {
        let files = [
            lang("B", "b", "Spanish", "x", ""),
            lang("A", "a", "German", "x", ""),
            lang("C", "c", "German", "y", ""),
        ];
        assert_eq!(bases(&files), vec!["German", "Spanish"]);
    }

    #[test]
    fn upload_needs_an_xml_document() {
        assert_eq!(check_upload_name(None), Err(MUST_REPLY));
        assert_eq!(check_upload_name(Some("German.txt")), Err(MUST_BE_XML));
        assert_eq!(check_upload_name(Some("German.XML")), Ok(()));
    }

    #[test]
    fn uploading_a_clean_update_compares_with_the_old_file() {
        let installed = [lang(
            "German",
            "Deutsch",
            "German",
            "Normal",
            r#"<string key="Joined"><value>x</value></string>"#,
        )];
        let mut new = lang("German", "Deutsch", "German", "Normal", COMPLETE);
        new.pack.meta.is_default = Some(true);
        let check = check_upload(&new, &installed, &master());
        assert!(check.can_upload);
        assert_eq!(
            check.report,
            "NEW FILE\n*German.xml - (Deutsch)*\n_No errors_\n\n\nOLD FILE (Last updated: Oct 03)\n\
             *German.xml - (Deutsch)*\nErrors: 1\nMissing strings: 2\n\n*isDefault changed from False to True*"
        );
    }

    #[test]
    fn uploading_a_new_language_asks_to_double_check() {
        let installed = [lang("German", "Deutsch", "German", "Normal", COMPLETE)];
        let new = lang("Klingon", "tlhIngan", "Klingon", "Normal", COMPLETE);
        let check = check_upload(&new, &installed, &master());
        assert!(check.can_upload);
        assert!(check.report.contains("*No old file, this is a new language*"));
        assert!(check.report.contains("\n_Base:_ Klingon *(NEW)*\n_Variant:_ Normal"));
    }

    #[test]
    fn uploads_that_clash_with_an_installed_language_are_fatal() {
        let installed = [lang("German", "Deutsch", "German", "Normal", COMPLETE)];
        for clash in [
            lang("german", "Anders", "German", "Other", COMPLETE),
            lang("Deutsch2", "Deutsch", "German", "Other", COMPLETE),
            lang("Deutsch2", "Anders", "German", "Normal", COMPLETE),
        ] {
            let check = check_upload(&clash, &installed, &master());
            assert!(!check.can_upload, "{}", clash.file_name);
            assert!(check.report.contains("partially matches the same language node"));
            assert!(check
                .report
                .contains("*German.xml*\n_Name:_Deutsch\n_Base:_German\n_Variant:_Normal"));
        }
    }

    #[test]
    fn ads_block_an_upload() {
        let new = lang(
            "German",
            "Deutsch",
            "German",
            "Normal",
            &COMPLETE.replace("Los!", "t.me/+x"),
        );
        let check = check_upload(&new, &[], &master());
        assert!(!check.can_upload);
        assert!(check
            .report
            .starts_with("NEW FILE\n*German.xml - (Deutsch)*\n*ADS DETECTED*\nStart\n"));
    }
}
