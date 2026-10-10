//! The language admin tools against the real language files, and the
//! install step on a scratch directory.
use i18n::admin::{self, ErrorLevel, LangFile};
use i18n::{LanguageId, LanguagePack};
use std::fs;
use std::path::PathBuf;

const LANGUAGES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Werewolf for Telegram/Languages");

fn master() -> LanguagePack {
    LanguagePack::load(format!("{LANGUAGES_DIR}/English.xml")).unwrap()
}

#[test]
fn english_passes_its_own_checks() {
    let english = LangFile::load(format!("{LANGUAGES_DIR}/English.xml")).unwrap();
    assert_eq!(admin::check_file(&english, &master()), vec![]);
}

#[test]
fn every_shipped_file_can_be_validated() {
    let (files, failures) = admin::load_dir(LANGUAGES_DIR).unwrap();
    assert!(failures.is_empty(), "{failures:?}");
    assert!(files.len() > 200);
    let (_details, summary) = admin::summary_report(&files, &master());
    assert!(summary.starts_with("*Validation complete*\nErrors: "));
    assert!(admin::bases(&files).contains(&"English"));
    // No shipped file has a fatal language-node problem. (The C#'s
    // callback-length check flagged "Singapore (Public Housing Version)";
    // buttons carry ids now, so that check is gone.)
    let fatal: Vec<_> = files
        .iter()
        .flat_map(|f| admin::check_file(f, &master()))
        .filter(|e| e.level == ErrorLevel::FatalError && e.key == "*Language Node*")
        .map(|e| e.file)
        .collect();
    assert!(fatal.is_empty(), "{fatal:?}");
}

#[test]
fn every_shipped_file_and_base_has_its_own_id() {
    let (files, _) = admin::load_dir(LANGUAGES_DIR).unwrap();
    // Files and bases are looked up separately, so a file named like its
    // base ("English") is fine; within each kind, ids must not collide.
    let distinct = |names: Vec<&str>| {
        let mut ids: Vec<LanguageId> = names.iter().map(|n| LanguageId::of(n)).collect();
        ids.sort();
        ids.dedup();
        ids.len() == names.len()
    };
    assert!(distinct(files.iter().map(|f| f.file_name.as_str()).collect()), "two file names share an id");
    assert!(distinct(admin::bases(&files)), "two bases share an id");
    let singapore = admin::file_by_id(&files, LanguageId::of("Singapore (Public Housing Version)"));
    assert!(singapore.is_some());
}

/// A fresh scratch directory per test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wolf-lang-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("upload")).unwrap();
    dir
}

fn write_lang(path: &PathBuf, name: &str, base: &str, variant: &str) {
    let xml = format!(
        r#"<strings><language name="{name}" base="{base}" variant="{variant}" /><string key="A"><value>a</value></string></strings>"#
    );
    fs::write(path, xml).unwrap();
}

#[test]
fn installing_replaces_the_file_with_the_same_language_name() {
    let dir = scratch("replace");
    write_lang(&dir.join("Deutsch.xml"), "Deutsch", "German", "Normal");
    let upload = dir.join("upload/German.xml");
    write_lang(&upload, "Deutsch", "German", "Normal");

    let installed = admin::install(&upload, &dir).unwrap();
    assert_eq!(installed.path, Some(dir.join("Deutsch.xml")));
    assert_eq!(
        installed.log,
        "Moving file to production..\nChecking paths for duplicate language file...\n\
         Found duplicate language (name attribute) with filename Deutsch\nFile copied to bot\n\n<b>Operation complete.</b>"
    );
    assert!(!upload.exists());
    assert!(!dir.join("German.xml").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn installing_a_new_language_adds_its_file() {
    let dir = scratch("new");
    write_lang(&dir.join("Deutsch.xml"), "Deutsch", "German", "Normal");
    let upload = dir.join("upload/Klingon.xml");
    write_lang(&upload, "tlhIngan", "Klingon", "Normal");

    let installed = admin::install(&upload, &dir).unwrap();
    assert_eq!(installed.path, Some(dir.join("Klingon.xml")));
    assert!(LangFile::load(dir.join("Klingon.xml")).is_ok());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn installing_over_another_languages_base_and_variant_is_refused() {
    let dir = scratch("clash");
    write_lang(&dir.join("Deutsch.xml"), "Deutsch", "German", "Normal");
    let upload = dir.join("upload/Other.xml");
    write_lang(&upload, "Anders", "German", "Normal");

    let installed = admin::install(&upload, &dir).unwrap();
    assert_eq!(installed.path, None);
    assert!(installed.log.ends_with("Found duplicate language (matching base and variant) with filename Deutsch\nAborting!"));
    assert!(!dir.join("Other.xml").exists());
    fs::remove_dir_all(dir).unwrap();
}
