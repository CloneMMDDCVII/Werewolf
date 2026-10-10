//! Moderation: who may run which command, global bans, and the admin
//! commands built on them (`/smite`, `/getban`, `/permban`, `/remban`).
//!
//! Fresh mechanism, preserved history: the legacy `GlobalBan` table is
//! retired, but its rows migrate in (`LegacyGlobalBan` into `Ban` with
//! `BanSource::Migrated`), so historic bans keep working without carrying
//! the old table shape or its coupling to the website's admin login.
//!
//! Nothing here talks to Telegram or a database: commands take an
//! already-parsed `CommandMessage` plus a `PlayerDirectory` and `BanList`,
//! and return the replies to send. Bans are still issued the way the C#
//! issued them (`/permban`); a redesign would replace that, not the rest.

pub mod ban;
pub mod commands;
pub mod gate;
pub mod target;
pub mod time;

pub use ban::{Ban, BanIssuer, BanList, BanSource, LegacyGlobalBan};
pub use commands::{escape_html, getban_report, issuer_name, permban, remban, smite_targets, BanReport};
pub use gate::{check, policy_for, BlockedChat, Chat, CommandPolicy, GateDecision, Invocation, Refusal, Sender};
pub use target::{resolve_target, CommandMessage, Mention, PlayerDirectory, PlayerRecord, ReplyTo};
pub use time::Timestamp;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupAdmin {
    pub group_telegram_id: i64,
    pub telegram_id: i64,
}
