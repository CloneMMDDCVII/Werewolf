//! Short, stable ids for language files and base languages, for Telegram
//! button data. The legacy bot put the names themselves into callback
//! data, which Telegram caps at 64 bytes, so a long file name or base and
//! variant could break the buttons (the C# guarded with upload-time length
//! checks, and one shipped file still failed them). An id is 16 hex
//! characters whatever the name, so no name can push a button over.
//!
//! The id is a hash of the name, not a position in a list: it stays valid
//! when files are added or removed, so a button sent yesterday still means
//! the same file today (or resolves to nothing if the file is gone).

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LanguageId(u64);

impl LanguageId {
    /// The id of a file name (without `.xml`) or of a base language name.
    /// 64-bit FNV-1a over the UTF-8 bytes: stable across builds and
    /// platforms, unlike `std`'s randomly seeded hasher.
    pub fn of(name: &str) -> Self {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in name.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        LanguageId(hash)
    }

    /// The name among `names` with this id, if any.
    pub fn find_in<'a, I>(self, names: I) -> Option<&'a str>
    where
        I: IntoIterator<Item = &'a str>,
    {
        names.into_iter().find(|name| LanguageId::of(name) == self)
    }
}

impl fmt::Display for LanguageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseLanguageIdError;

impl FromStr for LanguageId {
    type Err = ParseLanguageIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 16 {
            return Err(ParseLanguageIdError);
        }
        u64::from_str_radix(s, 16).map(LanguageId).map_err(|_| ParseLanguageIdError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable_and_fixed_width() {
        // Pinned (checked against a reference FNV-1a): changing the hash
        // would orphan every button already sent.
        assert_eq!(LanguageId::of("English").to_string(), "4caed5b7a7e5d89b");
        assert_eq!(LanguageId::of("").to_string(), "cbf29ce484222325");
        let long = LanguageId::of(&"x".repeat(500)).to_string();
        assert_eq!(long.len(), 16);
    }

    #[test]
    fn ids_round_trip_through_text() {
        let id = LanguageId::of("Singapore (Public Housing Version)");
        assert_eq!(id.to_string().parse::<LanguageId>(), Ok(id));
        assert!("xyz".parse::<LanguageId>().is_err());
        assert!("0123456789abcdefg".parse::<LanguageId>().is_err());
    }

    #[test]
    fn ids_resolve_back_to_their_name() {
        let names = ["English", "English Pirate", "Deutsch"];
        assert_eq!(LanguageId::of("Deutsch").find_in(names), Some("Deutsch"));
        assert_eq!(LanguageId::of("Klingon").find_in(names), None);
    }
}
