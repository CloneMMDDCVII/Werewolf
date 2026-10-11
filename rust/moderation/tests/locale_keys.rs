//! Every refusal the permission gate sends must name a key that exists in
//! English.xml, or players would see a missing-string fallback.
use i18n::LanguagePack;
use moderation::Refusal;

const LANGUAGE_PACK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../Werewolf for Telegram/Languages/English.xml"
);

#[test]
fn every_refusal_key_exists_in_english_xml() {
    let pack = LanguagePack::load(LANGUAGE_PACK_PATH).expect("English.xml should load");
    for key in Refusal::ALL_LOCALE_KEYS {
        assert!(pack.get(key).is_some(), "{key} is missing from English.xml");
    }
}
