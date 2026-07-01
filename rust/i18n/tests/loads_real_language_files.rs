use i18n::Catalog;

const LANGUAGES_DIR: &str =
    "/home/user/Werewolf/Werewolf for Telegram/Languages";

#[test]
fn loads_all_language_files_without_error() {
    let catalog = Catalog::load_dir(LANGUAGES_DIR).expect("catalog should load");
    assert!(catalog.len() > 10, "expected many language files, got {}", catalog.len());
    assert!(
        catalog.load_failures.is_empty(),
        "expected every language file to parse, but {} failed: {:?}",
        catalog.load_failures.len(),
        catalog.load_failures
    );
}

/// Some translations embed Telegram HTML formatting directly inside a
/// <value> (e.g. Persian Hip Hop.xml has `تو که توی بازی ای <b>گل من</b>!`).
/// That's legal mixed-content XML; the loader must preserve the inline
/// markup verbatim rather than choking on it or stripping it.
#[test]
fn preserves_inline_markup_in_values() {
    let catalog = Catalog::load_dir(LANGUAGES_DIR).expect("catalog should load");
    let pack = catalog
        .get("Persian Hip Hop")
        .expect("Persian Hip Hop.xml should load");
    let found = pack
        .variants_iter()
        .any(|v| v.contains("<b>") && v.contains("</b>"));
    assert!(found, "expected at least one value to retain <b>...</b> markup");
}

#[test]
fn english_has_expected_keys_and_variants() {
    let catalog = Catalog::load_dir(LANGUAGES_DIR).expect("catalog should load");
    let en = catalog.get("English").expect("English.xml should load");
    assert_eq!(en.meta.code.as_deref(), Some("en"));
    assert_eq!(en.meta.is_default, Some(true));

    let variants = en
        .variants("PlayerStartedGame")
        .expect("PlayerStartedGame should exist");
    assert_eq!(variants.len(), 2, "PlayerStartedGame has 2 value variants in source XML");

    assert!(en.get("MinuteLeftToJoin").is_some());
    // NotInGame is marked deprecated="true" in source and should be excluded
    // from active strings, but tracked separately.
    assert!(en.get("NotInGame").is_none());
    assert!(en.deprecated_keys.contains(&"NotInGame".to_string()));
}
