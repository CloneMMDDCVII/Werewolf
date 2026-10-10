//! The moderation commands on real storage: bans persist through
//! `db::Store` (history kept, nothing deleted), and an in-memory
//! `BanList` answers "is this sender banned" on every command without a
//! query, as `UpdateHandler.BanList` did.
//!
//! Each command first loads every player the message could name into a
//! small in-memory directory, then runs the pure logic from `commands`,
//! `idles` and `restore` over it.

use crate::ban::{Ban, BanIssuer, BanList, BanSource};
use crate::commands::{getban_report, permban, remban, smite_targets, BanReport};
use crate::idles::{getidles_report, getidles_targets, IdleCount, IDLE_WINDOW_SECS};
use crate::restore::{parse_restore_args, restore_report, restored_reply, AccountFacts, RestoreCheck};
use crate::target::{resolve_target, CommandMessage, Mention, PlayerDirectory, PlayerRecord};
use shared::time::Timestamp;
use db::{DbResult, Store};
use i18n::LanguagePack;

pub struct Moderation {
    store: Store,
    bans: BanList,
}

impl Moderation {
    /// Loads every ban in force at `now` into memory.
    pub async fn load(store: Store, now: Timestamp) -> DbResult<Self> {
        let mut bans = BanList::new();
        for record in store.bans().all_active(now.0).await? {
            let ban = ban_from_db(record);
            // With overlapping bans the longest one decides, as in
            // `db::Bans::active`.
            let longer = bans
                .active(ban.telegram_id, now)
                .is_none_or(|held| held.expires_at.is_some_and(|t| ban.expires_at.is_none_or(|u| u > t)));
            if longer {
                bans.issue(ban);
            }
        }
        Ok(Moderation { store, bans })
    }

    pub fn bans(&self) -> &BanList {
        &self.bans
    }

    pub fn is_banned(&self, telegram_id: i64, now: Timestamp) -> bool {
        self.bans.is_banned(telegram_id, now)
    }

    pub async fn smite_targets(&self, message: &CommandMessage) -> DbResult<Vec<i64>> {
        Ok(smite_targets(message, &self.directory_for(message).await?))
    }

    /// `None` when neither the target nor the sender is a known player
    /// (the C# crashed there; the port stays quiet).
    pub async fn getban(&self, message: &CommandMessage, now: Timestamp) -> DbResult<Option<String>> {
        let players = self.directory_for(message).await?;
        let Some(target) = resolve_target(message, &players) else {
            return Ok(None);
        };
        let ban = self.bans.active(target.telegram_id, now);
        let issuer = match ban.map(|b| &b.issued_by) {
            Some(BanIssuer::Admin { telegram_id }) => self.fetch(*telegram_id).await?,
            _ => None,
        };
        let players = Directory(players.0.into_iter().chain(issuer).collect());
        let first_seen = self.store.games().first_started(target.telegram_id).await?.map(Timestamp);
        Ok(Some(getban_report(&target, ban, &players, first_seen, now)))
    }

    /// Bans as the sender, who must already have passed the gate.
    pub async fn permban(&mut self, message: &CommandMessage, now: Timestamp) -> DbResult<BanReport> {
        let players = self.directory_for(message).await?;
        let issuer = BanIssuer::Admin { telegram_id: message.sender_id };
        let report = permban(message, &issuer, &players, now);
        for player in &report.new_players {
            self.store.players().upsert(player.telegram_id, &player.name, player.username.as_deref(), now.0).await?;
        }
        for ban in &report.issued {
            self.store.bans().insert(&new_ban(ban)).await?;
            self.bans.issue(ban.clone());
        }
        Ok(report)
    }

    pub async fn remban(&mut self, message: &CommandMessage, now: Timestamp) -> DbResult<BanReport> {
        let players = self.directory_for(message).await?;
        let report = remban(message, &players, &self.bans, now);
        for id in &report.lifted {
            self.store.bans().lift(*id, message.sender_id, now.0).await?;
            self.bans.lift(*id);
        }
        Ok(report)
    }

    /// `group` is the chat the command was sent in.
    pub async fn getidles(
        &self,
        message: &CommandMessage,
        group: i64,
        now: Timestamp,
        pack: &LanguagePack,
    ) -> DbResult<String> {
        let since = now.0 - IDLE_WINDOW_SECS;
        let games = self.store.games();
        let mut counts = vec![];
        for telegram_id in getidles_targets(message) {
            counts.push(IdleCount {
                telegram_id,
                name: self.fetch(telegram_id).await?.map(|p| p.name),
                total: games.idle_kills_since(telegram_id, since, None).await?,
                in_group: games.idle_kills_since(telegram_id, since, Some(group)).await?,
            });
        }
        Ok(getidles_report(&counts, pack))
    }

    pub async fn restore_check(&self, argument: Option<&str>, now: Timestamp) -> DbResult<RestoreCheck> {
        let Some((old, new)) = parse_restore_args(argument) else {
            return Ok(RestoreCheck::Usage);
        };
        let (Some(old_facts), Some(new_facts)) = (self.account(old).await?, self.account(new).await?) else {
            return Ok(RestoreCheck::NotFound);
        };
        Ok(restore_report(&old_facts, &new_facts, self.is_banned(old, now)))
    }

    /// Runs once a dev confirms a `RestoreCheck::Ready`.
    pub async fn restore(&self, old: i64, new: i64) -> DbResult<String> {
        self.store.games().move_player_history(old, new).await?;
        let name = |p: Option<PlayerRecord>| p.map(|p| p.name).unwrap_or_default();
        Ok(restored_reply(&name(self.fetch(old).await?), &name(self.fetch(new).await?)))
    }

    async fn account(&self, telegram_id: i64) -> DbResult<Option<AccountFacts>> {
        let Some(player) = self.store.players().get(telegram_id).await? else {
            return Ok(None);
        };
        let games = self.store.games().games_of(telegram_id).await?;
        let mut group_ids: Vec<i64> = games.iter().map(|(_, group)| *group).collect();
        group_ids.sort_unstable();
        group_ids.dedup();
        Ok(Some(AccountFacts {
            telegram_id,
            name: player.name,
            username: player.username,
            language: player.language,
            game_ids: games.iter().map(|(game, _)| game.0).collect(),
            group_ids,
        }))
    }

    async fn fetch(&self, telegram_id: i64) -> DbResult<Option<PlayerRecord>> {
        Ok(self.store.players().get(telegram_id).await?.map(player_from_db))
    }

    /// Every player `message` could refer to, by id or by username.
    async fn directory_for(&self, message: &CommandMessage) -> DbResult<Directory> {
        let mut ids = vec![message.sender_id];
        if let Some(reply) = message.reply_to {
            ids.push(reply.from);
            ids.extend(reply.forwarded_from);
        }
        let mut usernames = vec![];
        for mention in &message.mentions {
            match mention {
                Mention::Username(name) => usernames.push(name.clone()),
                Mention::User(user) => ids.push(user.telegram_id),
            }
        }
        for word in message.argument_words() {
            match word.parse::<i64>() {
                Ok(id) => ids.push(id),
                Err(_) => usernames.push(word.trim_start_matches('@').to_string()),
            }
        }
        ids.sort_unstable();
        ids.dedup();

        let mut found = vec![];
        for id in ids {
            found.extend(self.fetch(id).await?);
        }
        for name in usernames {
            if let Some(p) = self.store.players().by_username(&name).await? {
                found.push(player_from_db(p));
            }
        }
        Ok(Directory(found))
    }
}

/// The players one command can see, loaded up front.
struct Directory(Vec<PlayerRecord>);

impl PlayerDirectory for Directory {
    fn by_id(&self, telegram_id: i64) -> Option<PlayerRecord> {
        self.0.iter().find(|p| p.telegram_id == telegram_id).cloned()
    }
    fn by_username(&self, username: &str) -> Option<PlayerRecord> {
        self.0
            .iter()
            .find(|p| p.username.as_deref().is_some_and(|u| u.eq_ignore_ascii_case(username)))
            .cloned()
    }
}

fn player_from_db(p: db::PlayerRecord) -> PlayerRecord {
    PlayerRecord { telegram_id: p.telegram_id, name: p.name, username: p.username }
}

fn ban_from_db(r: db::BanRecord) -> Ban {
    Ban {
        telegram_id: r.telegram_id,
        reason: r.reason,
        issued_by: match r.issued_by {
            db::BanIssuer::Admin { telegram_id } => BanIssuer::Admin { telegram_id },
            db::BanIssuer::System { rule } => BanIssuer::System { rule },
        },
        issued_at: Timestamp(r.issued_at),
        expires_at: r.expires_at.map(Timestamp),
        source: match r.source {
            db::BanSource::Migrated => BanSource::Migrated,
            db::BanSource::Native => BanSource::Native,
        },
    }
}

fn new_ban(ban: &Ban) -> db::NewBan {
    db::NewBan {
        telegram_id: ban.telegram_id,
        reason: ban.reason.clone(),
        issued_by: match &ban.issued_by {
            BanIssuer::Admin { telegram_id } => db::BanIssuer::Admin { telegram_id: *telegram_id },
            BanIssuer::System { rule } => db::BanIssuer::System { rule: rule.clone() },
        },
        issued_at: ban.issued_at.0,
        expires_at: ban.expires_at.map(|t| t.0),
        source: match ban.source {
            BanSource::Migrated => db::BanSource::Migrated,
            BanSource::Native => db::BanSource::Native,
        },
    }
}
