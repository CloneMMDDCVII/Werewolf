//! Measures how much of the legacy game's actual message catalog
//! (`Languages/English.xml`) this port currently reaches, as a real
//! number rather than a vibe — the same "exhaustive match, no silent
//! wildcard" instinct behind `roles::behavior_for`, applied to locale
//! keys instead of roles.
//!
//! This test does **not** yet assert full coverage — categorizing all
//! ~630 keys into "mapped" / "legacy feature we're not porting" /
//! "still to do" is real, incremental work (transforms, achievements,
//! per-role death flavor, lobby/settings text), not something to fake a
//! green check for today. What it *does* assert, and can fail hard on:
//!
//! 1. Every key `game_engine::orchestrator::all_mapped_locale_keys()`
//!    claims to use actually exists in English.xml — catches a typo'd
//!    key name immediately, the same day it's introduced, rather than
//!    letting it silently fall back to generic wording forever (exactly
//!    how the Fool/Sorcerer/Oracle mismap went unnoticed before someone
//!    actually looked at the output).
//! 2. The coverage number itself, printed so it's visible in test output
//!    - a deliberately un-hidden TODO count, not swept into a comment
//!    that goes stale.
use i18n::LanguagePack;
use std::collections::HashSet;

const LANGUAGE_PACK_PATH: &str = "/home/user/Werewolf/Werewolf for Telegram/Languages/English.xml";

#[test]
fn every_mapped_locale_key_actually_exists_in_english_xml() {
    let pack = LanguagePack::load(LANGUAGE_PACK_PATH).expect("English.xml should load");
    let all_keys: HashSet<&str> = pack.keys().collect();

    let mapped = game_engine::orchestrator::all_mapped_locale_keys();
    assert!(!mapped.is_empty(), "expected at least one mapped locale key");

    let missing: Vec<&str> = mapped.iter().filter(|k| !all_keys.contains(*k)).copied().collect();
    assert!(
        missing.is_empty(),
        "these keys are claimed by prompt_locale_key/death_locale_key but don't exist in \
         English.xml (typo, or the key was renamed upstream): {missing:?}"
    );

    let total = all_keys.len();
    let covered = mapped.len();
    println!(
        "locale coverage: {covered}/{total} English.xml keys mapped to a Prompt/NarrationEvent \
         ({} not yet categorized as mapped or explicitly out of scope)",
        total - covered
    );
}
