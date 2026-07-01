//! Moderation: fresh mechanism, preserved history.
//!
//! The legacy schema (`GlobalBan`, `GroupAdmin`, `ContestTerms` in
//! werewolf.sql) is retired as-is, but its data is migrated in — a
//! `GlobalBan` row becomes a `Ban` with `source: BanSource::Migrated`, so
//! historic bans keep working without carrying forward the old table
//! shape or its coupling to the website's admin login flow.
//!
//! Not yet implemented: the actual redesign of *how* bans get issued
//! (currently pending — this crate only fixes the data model and
//! migration path so that work has somewhere to land).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ban {
    pub telegram_id: i64,
    pub reason: String,
    pub issued_by: BanIssuer,
    pub issued_at: String,
    pub expires_at: Option<String>,
    pub source: BanSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BanIssuer {
    Admin { telegram_id: i64 },
    System { rule: String },
}

/// Distinguishes bans carried over from the legacy DB from ones issued
/// under the new mechanism, so we can tell them apart during/after migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BanSource {
    Migrated,
    Native,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupAdmin {
    pub group_telegram_id: i64,
    pub telegram_id: i64,
}
