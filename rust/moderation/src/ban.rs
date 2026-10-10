//! Global bans: the record, where it came from, and the in-memory list
//! the bot consults on every command (`UpdateHandler.BanList`).

use shared::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ban {
    pub telegram_id: i64,
    pub reason: String,
    pub issued_by: BanIssuer,
    pub issued_at: Timestamp,
    /// `None` is a permanent ban.
    pub expires_at: Option<Timestamp>,
    pub source: BanSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BanIssuer {
    /// Issued in this port by a global admin (`/permban`).
    Admin { telegram_id: i64 },
    /// Issued automatically (the C# spam filter, UpdateHandler.cs:99), or
    /// a migrated row, where `rule` holds the issuer's first name: that is
    /// all the C# `BannedBy` column ever stored.
    System { rule: String },
}

/// Distinguishes bans carried over from the legacy DB from ones issued
/// under the new mechanism, so we can tell them apart during/after migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BanSource {
    Migrated,
    Native,
}

/// One row of the legacy `GlobalBan` table, as the migration reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyGlobalBan {
    pub telegram_id: i64,
    pub reason: String,
    pub expires: Timestamp,
    pub banned_by: String,
    pub ban_date: Option<Timestamp>,
}

impl From<LegacyGlobalBan> for Ban {
    /// Permanent legacy bans were written as `SqlDateTime.MaxValue`
    /// (DevCommands.cs:876); `/getbans` split permanent from temporary at
    /// the year 3000 (DevCommands.cs:831, 840), so the migration does too.
    fn from(row: LegacyGlobalBan) -> Self {
        Ban {
            telegram_id: row.telegram_id,
            reason: row.reason,
            issued_by: BanIssuer::System { rule: row.banned_by },
            // Nullable in the legacy schema; the epoch marks "unknown".
            issued_at: row.ban_date.unwrap_or(Timestamp(0)),
            expires_at: (row.expires.year() < 3000).then_some(row.expires),
            source: BanSource::Migrated,
        }
    }
}

impl Ban {
    pub fn is_active(&self, now: Timestamp) -> bool {
        self.expires_at.is_none_or(|expires| expires > now)
    }
}

/// Every known ban, one per player.
///
/// The C# could hold several `GlobalBan` rows for one player (`/permban`
/// always added a row) and `/getban` and `/remban` then acted on
/// whichever came first. Here a new ban replaces the old one, so there is
/// only ever one answer to "is this player banned and why".
#[derive(Debug, Clone, Default)]
pub struct BanList {
    bans: HashMap<i64, Ban>,
}

impl BanList {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `ban`, returning the ban it replaced, if any.
    pub fn issue(&mut self, ban: Ban) -> Option<Ban> {
        self.bans.insert(ban.telegram_id, ban)
    }

    /// Removes the player's ban, active or expired.
    pub fn lift(&mut self, telegram_id: i64) -> Option<Ban> {
        self.bans.remove(&telegram_id)
    }

    /// The player's ban, if it is still in force at `now`.
    ///
    /// Deliberate change: the C# `/getban` read the database directly and
    /// showed an expired ban (with negative time left) until the next
    /// refresh pruned it (UpdateHandler.cs:113-121). Here an expired ban is
    /// simply not a ban.
    pub fn active(&self, telegram_id: i64, now: Timestamp) -> Option<&Ban> {
        self.bans.get(&telegram_id).filter(|ban| ban.is_active(now))
    }

    pub fn is_banned(&self, telegram_id: i64, now: Timestamp) -> bool {
        self.active(telegram_id, now).is_some()
    }

    /// Drops every ban that has expired by `now` (the C#'s periodic
    /// refresh, UpdateHandler.cs:113-121), returning how many went.
    pub fn prune_expired(&mut self, now: Timestamp) -> usize {
        let before = self.bans.len();
        self.bans.retain(|_, ban| ban.is_active(now));
        before - self.bans.len()
    }

    pub fn len(&self) -> usize {
        self.bans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bans.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Timestamp {
        Timestamp::from_utc(2026, 10, 10, 12, 0, 0)
    }

    fn ban(id: i64, expires_at: Option<Timestamp>) -> Ban {
        Ban {
            telegram_id: id,
            reason: "spam".into(),
            issued_by: BanIssuer::Admin { telegram_id: 1 },
            issued_at: now(),
            expires_at,
            source: BanSource::Native,
        }
    }

    #[test]
    fn a_permanent_ban_is_always_active() {
        let mut list = BanList::new();
        list.issue(ban(7, None));
        assert!(list.is_banned(7, now()));
        assert!(list.is_banned(7, Timestamp::from_utc(2999, 1, 1, 0, 0, 0)));
    }

    #[test]
    fn an_expired_ban_no_longer_counts() {
        let mut list = BanList::new();
        list.issue(ban(7, Some(now().plus_seconds(-86_400))));
        assert!(!list.is_banned(7, now()));
        assert!(list.active(7, now()).is_none());
    }

    #[test]
    fn a_ban_expiring_exactly_now_has_ended() {
        let mut list = BanList::new();
        list.issue(ban(7, Some(now())));
        assert!(!list.is_banned(7, now()));
    }

    #[test]
    fn issuing_again_replaces_the_old_ban() {
        let mut list = BanList::new();
        list.issue(ban(7, Some(now().plus_seconds(60))));
        let replaced = list.issue(ban(7, None));
        assert!(replaced.is_some());
        assert_eq!(list.len(), 1);
        assert_eq!(list.active(7, now()).unwrap().expires_at, None);
    }

    #[test]
    fn lifting_removes_the_ban() {
        let mut list = BanList::new();
        list.issue(ban(7, None));
        assert!(list.lift(7).is_some());
        assert!(list.lift(7).is_none());
        assert!(!list.is_banned(7, now()));
    }

    #[test]
    fn pruning_keeps_only_bans_still_in_force() {
        let mut list = BanList::new();
        list.issue(ban(1, None));
        list.issue(ban(2, Some(now().plus_seconds(-1))));
        list.issue(ban(3, Some(now().plus_seconds(1))));
        assert_eq!(list.prune_expired(now()), 1);
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn migrated_max_value_bans_become_permanent() {
        let row = LegacyGlobalBan {
            telegram_id: 9,
            reason: "cheating".into(),
            expires: Timestamp::from_utc(9999, 12, 31, 23, 59, 59),
            banned_by: "Para".into(),
            ban_date: None,
        };
        let ban = Ban::from(row);
        assert_eq!(ban.expires_at, None);
        assert_eq!(ban.source, BanSource::Migrated);
        assert_eq!(ban.issued_by, BanIssuer::System { rule: "Para".into() });
        assert_eq!(ban.issued_at, Timestamp(0));
    }

    #[test]
    fn migrated_temporary_bans_keep_their_expiry() {
        let expires = Timestamp::from_utc(2026, 11, 1, 0, 0, 0);
        let row = LegacyGlobalBan {
            telegram_id: 9,
            reason: "afk".into(),
            expires,
            banned_by: "Para".into(),
            ban_date: Some(now()),
        };
        assert_eq!(Ban::from(row).expires_at, Some(expires));
    }
}
