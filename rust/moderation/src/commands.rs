//! `/smite`, `/getban`, `/permban` and `/remban`, minus the Telegram I/O:
//! each takes an already-parsed [`CommandMessage`] and returns what to
//! send and what to do, so the presenter only has to deliver it.

use crate::ban::{Ban, BanIssuer, BanList, BanSource};
use crate::target::{CommandMessage, Mention, PlayerDirectory, PlayerRecord};
use crate::time::Timestamp;

/// `FormatHTML` (Extensions.cs:28-31): every player-supplied string that
/// goes into an HTML-mode message passes through this.
pub fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Who `/smite` removes from the group's game (AdminCommands.cs:26-63), in
/// order: the author of the message replied to, then each mention, then a
/// single numeric id typed as the whole argument. Unknown usernames are
/// skipped silently, as in the C#. Each id appears once.
///
/// Smite itself is a forced flee: the caller removes each id from the
/// group's lobby (`control::lobby::Lobby::leave`) or reports it through
/// `Presenter::drain_fled_players` in a running game, the same path
/// `/flee` takes (`Werewolf.cs::FleePlayer`, 5357-5391). Unlike `/flee`
/// it skips the group's "allow flee" setting, and says nothing about a
/// target who is already dead or isn't playing.
pub fn smite_targets(message: &CommandMessage, players: &dyn PlayerDirectory) -> Vec<i64> {
    let mut ids = Vec::new();
    if let Some(reply) = message.reply_to {
        ids.push(reply.from);
    }
    for mention in &message.mentions {
        match mention {
            Mention::Username(name) => ids.extend(players.by_username(name).map(|p| p.telegram_id)),
            Mention::User(user) => ids.push(user.telegram_id),
        }
    }
    // `long.TryParse(args[1])`: only an argument that is one id, nothing else.
    if let Some(id) = message.argument.as_deref().and_then(|a| a.trim().parse::<i64>().ok()) {
        ids.push(id);
    }
    let mut seen = std::collections::HashSet::new();
    ids.retain(|id| seen.insert(*id));
    ids
}

/// Bans with more than a year left are shown as permanent
/// (AdminCommands.cs:141).
const PERM_BAN_THRESHOLD_SECS: i64 = 365 * 86_400;

/// What `/getban` prints after "By:" (the C# `BannedBy` column): the
/// issuing admin's name, or the rule (or migrated issuer name) otherwise.
pub fn issuer_name(issuer: &BanIssuer, players: &dyn PlayerDirectory) -> String {
    match issuer {
        BanIssuer::Admin { telegram_id } => {
            players.by_id(*telegram_id).map_or_else(|| telegram_id.to_string(), |p| p.name)
        }
        BanIssuer::System { rule } => rule.clone(),
    }
}

/// The `/getban` reply (AdminCommands.cs:131-158). `ban` is the player's
/// active ban (see `BanList::active`), and `first_seen` the start of their
/// first game, if they have played.
pub fn getban_report(
    player: &PlayerRecord,
    ban: Option<&Ban>,
    players: &dyn PlayerDirectory,
    first_seen: Option<Timestamp>,
    now: Timestamp,
) -> String {
    let status = match ban {
        None => "Not banned (in Werewolf)".to_string(),
        Some(ban) => {
            let mut status = format!(
                "<b>Banned for: {}</b>\nBy: {} on {}\n",
                escape_html(&ban.reason),
                escape_html(&issuer_name(&ban.issued_by, players)),
                ban.issued_at.legacy_format(),
            );
            match ban.expires_at.map(|expires| expires.0 - now.0) {
                Some(left) if left <= PERM_BAN_THRESHOLD_SECS => status.push_str(&format!(
                    "Ban expiration: <b>{} days, {} hours, {} minutes</b>",
                    left / 86_400,
                    (left % 86_400) / 3600,
                    (left % 3600) / 60
                )),
                _ => status.push_str("<b>Perm Ban</b>"),
            }
            status
        }
    };
    let first_seen = first_seen.map_or_else(|| "Hasn't played ever!".to_string(), Timestamp::legacy_format);
    format!("Player: {}\nCurrent Status: {status}\nPlayer first seen: {first_seen}", escape_html(&player.name))
}

/// What a ban command decided. Nothing is stored yet: the caller records
/// `issued` and `lifted` (see `service::Moderation`, which also keeps the
/// in-memory `BanList` in step) and sends `replies` in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BanReport {
    pub replies: Vec<String>,
    /// `/permban`: who to smite from this group's game.
    pub smite: Vec<i64>,
    pub issued: Vec<Ban>,
    /// `/permban` of someone the bot has never seen: the player record to
    /// create, as the C# did (DevCommands.cs:901-910, 939-947).
    pub new_players: Vec<PlayerRecord>,
    /// `/remban`: whose ban to lift.
    pub lifted: Vec<i64>,
}

/// `/permban` (DevCommands.cs:848-980): permanently bans each mentioned
/// player and each numeric id, and smites them from the group's game.
///
/// The reason is every word of the argument that isn't a target. The C#
/// instead dropped the first word, which mangled the reason whenever more
/// than one target was given, and for typed ids it built the right reason
/// and then didn't use it. Text mentions are matched against the
/// argument by the mentioned user's name, so the presenter can pass the
/// message text through unchanged.
pub fn permban(message: &CommandMessage, issuer: &BanIssuer, players: &dyn PlayerDirectory, now: Timestamp) -> BanReport {
    let reason = ban_reason(message);
    let mut report = BanReport::default();
    let ban = |player: &PlayerRecord, report: &mut BanReport| {
        report.smite.push(player.telegram_id);
        report.issued.push(Ban {
            telegram_id: player.telegram_id,
            reason: reason.clone(),
            issued_by: issuer.clone(),
            issued_at: now,
            expires_at: None,
            source: BanSource::Native,
        });
    };

    for mention in &message.mentions {
        match mention {
            Mention::Username(name) => match players.by_username(name) {
                None => report.replies.push(format!("Unknown user: @{name}")),
                Some(player) => {
                    ban(&player, &mut report);
                    report.replies.push("User has been banned".into());
                }
            },
            Mention::User(user) => {
                if players.by_id(user.telegram_id).is_none() {
                    report.new_players.push(user.clone());
                }
                ban(user, &mut report);
                report.replies.push("User has been banned".into());
            }
        }
    }
    for id in message.numeric_ids() {
        let player = players.by_id(id).unwrap_or_else(|| {
            let unknown = PlayerRecord { telegram_id: id, name: "Unknown User".into(), username: None };
            report.new_players.push(unknown.clone());
            unknown
        });
        ban(&player, &mut report);
        let username = player.username.as_deref().unwrap_or("");
        report.replies.push(format!("User {} (@{username}) has been banned", player.name));
    }
    report
}

fn ban_reason(message: &CommandMessage) -> String {
    let mut argument = message.argument.clone().unwrap_or_default();
    for mention in &message.mentions {
        if let Mention::User(user) = mention {
            argument = argument.replacen(&user.name, " ", 1);
        }
    }
    argument
        .split_whitespace()
        .filter(|word| !word.starts_with('@') && word.parse::<i64>().is_err())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `/remban` (DevCommands.cs:982-1055): lifts the ban on each mentioned
/// player and each numeric id. A target with no ban in force gets no reply.
pub fn remban(message: &CommandMessage, players: &dyn PlayerDirectory, bans: &BanList, now: Timestamp) -> BanReport {
    let mention_ids = message.mentions.iter().filter_map(|mention| match mention {
        Mention::Username(name) => players.by_username(name).map(|p| p.telegram_id),
        Mention::User(user) => Some(user.telegram_id),
    });
    let mut lifted: Vec<i64> = vec![];
    for id in mention_ids.chain(message.numeric_ids()) {
        if bans.is_banned(id, now) && !lifted.contains(&id) {
            lifted.push(id);
        }
    }
    BanReport { replies: vec!["User has been unbanned.".to_string(); lifted.len()], lifted, ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::tests::{player, roster};
    use crate::target::ReplyTo;

    fn now() -> Timestamp {
        Timestamp::from_utc(2026, 10, 10, 12, 0, 0)
    }

    fn alice() -> BanIssuer {
        BanIssuer::Admin { telegram_id: 1 }
    }

    fn command(argument: &str, mentions: Vec<Mention>) -> CommandMessage {
        CommandMessage { sender_id: 1, reply_to: None, mentions, argument: Some(argument.into()) }
    }

    fn ban_until(expires_at: Option<Timestamp>) -> Ban {
        Ban {
            telegram_id: 2,
            reason: "spam".into(),
            issued_by: alice(),
            issued_at: Timestamp::from_utc(2026, 10, 1, 12, 0, 0),
            expires_at,
            source: BanSource::Native,
        }
    }

    fn apply(bans: &mut BanList, report: &BanReport) {
        for ban in &report.issued {
            bans.issue(ban.clone());
        }
        for id in &report.lifted {
            bans.lift(*id);
        }
    }

    // ---- smite ----

    #[test]
    fn smite_collects_reply_mentions_and_id_in_order() {
        let message = CommandMessage {
            sender_id: 1,
            reply_to: Some(ReplyTo { from: 5, forwarded_from: Some(6) }),
            mentions: vec![Mention::Username("paul".into()), Mention::User(player(3, "Rita", None))],
            argument: None,
        };
        assert_eq!(smite_targets(&message, &roster()), vec![5, 2, 3]);
    }

    #[test]
    fn smite_by_a_single_id() {
        assert_eq!(smite_targets(&command(" 42 ", vec![]), &roster()), vec![42]);
    }

    #[test]
    fn smite_ignores_ids_mixed_with_other_words() {
        assert!(smite_targets(&command("42 43", vec![]), &roster()).is_empty());
    }

    #[test]
    fn smite_skips_unknown_usernames_and_duplicates() {
        let message = CommandMessage {
            reply_to: Some(ReplyTo { from: 2, forwarded_from: None }),
            ..command("@nobody @paul", vec![Mention::Username("nobody".into()), Mention::Username("paul".into())])
        };
        assert_eq!(smite_targets(&message, &roster()), vec![2]);
    }

    // ---- getban ----

    #[test]
    fn getban_shows_a_temporary_ban() {
        let expires = now().plus_seconds(3 * 86_400 + 4 * 3600 + 5 * 60 + 30);
        let report = getban_report(&player(2, "Paul", None), Some(&ban_until(Some(expires))), &roster(), None, now());
        assert_eq!(
            report,
            "Player: Paul\nCurrent Status: <b>Banned for: spam</b>\nBy: Alice on 01OCT2026 12:00:00 +00:00\n\
             Ban expiration: <b>3 days, 4 hours, 5 minutes</b>\nPlayer first seen: Hasn't played ever!"
        );
    }

    #[test]
    fn getban_names_an_unknown_admin_by_id_and_a_migrated_issuer_by_name() {
        let mut ban = ban_until(None);
        ban.issued_by = BanIssuer::Admin { telegram_id: 404 };
        assert!(getban_report(&player(2, "Paul", None), Some(&ban), &roster(), None, now()).contains("By: 404 on"));
        ban.issued_by = BanIssuer::System { rule: "Para".into() };
        assert!(getban_report(&player(2, "Paul", None), Some(&ban), &roster(), None, now()).contains("By: Para on"));
    }

    #[test]
    fn getban_shows_far_off_and_permanent_bans_as_perm() {
        let far = ban_until(Some(now().plus_seconds(400 * 86_400)));
        assert!(getban_report(&player(2, "Paul", None), Some(&far), &roster(), None, now()).contains("<b>Perm Ban</b>"));
        let perm = ban_until(None);
        assert!(getban_report(&player(2, "Paul", None), Some(&perm), &roster(), None, now()).contains("<b>Perm Ban</b>"));
    }

    #[test]
    fn getban_without_a_ban() {
        let first = Timestamp::from_utc(2017, 3, 4, 18, 30, 0);
        assert_eq!(
            getban_report(&player(3, "Rita", None), None, &roster(), Some(first), now()),
            "Player: Rita\nCurrent Status: Not banned (in Werewolf)\nPlayer first seen: 04MAR2017 18:30:00 +00:00"
        );
    }

    #[test]
    fn getban_reports_an_expired_ban_as_not_banned() {
        let mut bans = BanList::new();
        bans.issue(ban_until(Some(now().plus_seconds(-60))));
        let report = getban_report(&player(2, "Paul", None), bans.active(2, now()), &roster(), None, now());
        assert!(report.contains("Not banned (in Werewolf)"));
    }

    #[test]
    fn getban_escapes_names_and_reasons() {
        let mut ban = ban_until(None);
        ban.reason = "said <script>".into();
        let report = getban_report(&player(2, "<b>x</b>", None), Some(&ban), &roster(), None, now());
        assert!(report.starts_with("Player: &lt;b&gt;x&lt;/b&gt;\n"));
        assert!(report.contains("said &lt;script&gt;"));
    }

    // ---- permban ----

    #[test]
    fn permban_by_id_bans_permanently_and_smites() {
        let report = permban(&command("2 cheating in games", vec![]), &alice(), &roster(), now());
        assert_eq!(report.replies, vec!["User Paul (@paul) has been banned"]);
        assert_eq!(report.smite, vec![2]);
        assert!(report.new_players.is_empty());
        let ban = &report.issued[0];
        assert_eq!(ban.telegram_id, 2);
        assert_eq!(ban.reason, "cheating in games");
        assert_eq!(ban.expires_at, None);
        assert_eq!(ban.source, BanSource::Native);
        assert_eq!(ban.issued_by, alice());
    }

    #[test]
    fn permban_of_a_never_seen_id_creates_the_player() {
        let report = permban(&command("123 spam", vec![]), &alice(), &roster(), now());
        assert_eq!(report.replies, vec!["User Unknown User (@) has been banned"]);
        assert_eq!(report.new_players, vec![player(123, "Unknown User", None)]);
    }

    #[test]
    fn permban_by_mention() {
        let message = command("@PAUL spam", vec![Mention::Username("PAUL".into())]);
        let report = permban(&message, &alice(), &roster(), now());
        assert_eq!(report.replies, vec!["User has been banned"]);
        assert_eq!(report.issued[0].reason, "spam");
    }

    #[test]
    fn permban_by_text_mention_drops_their_name_from_the_reason() {
        let stranger = player(77, "Mary Jane", None);
        let message = command("Mary Jane flooding the chat", vec![Mention::User(stranger.clone())]);
        let report = permban(&message, &alice(), &roster(), now());
        assert_eq!(report.smite, vec![77]);
        assert_eq!(report.issued[0].reason, "flooding the chat");
        assert_eq!(report.new_players, vec![stranger]);
    }

    #[test]
    fn permban_of_an_unknown_username_stores_nothing() {
        let message = command("@nobody spam", vec![Mention::Username("nobody".into())]);
        let report = permban(&message, &alice(), &roster(), now());
        assert_eq!(report.replies, vec!["Unknown user: @nobody"]);
        assert!(report.smite.is_empty() && report.issued.is_empty());
    }

    #[test]
    fn permban_with_several_targets_keeps_the_whole_reason() {
        let report = permban(&command("2 3 alt accounts", vec![]), &alice(), &roster(), now());
        assert!(report.issued.iter().all(|b| b.reason == "alt accounts"));
        assert_eq!(report.issued.len(), 2);
    }

    // ---- remban ----

    #[test]
    fn remban_lifts_by_mention_and_id() {
        let mut bans = BanList::new();
        apply(&mut bans, &permban(&command("2 3 spam", vec![]), &alice(), &roster(), now()));
        let message = command("@paul 3 2", vec![Mention::Username("paul".into())]);
        let report = remban(&message, &roster(), &bans, now());
        assert_eq!(report.lifted, vec![2, 3]);
        assert_eq!(report.replies, vec!["User has been unbanned.", "User has been unbanned."]);
        apply(&mut bans, &report);
        assert!(bans.is_empty());
    }

    #[test]
    fn remban_without_a_ban_says_nothing() {
        let report = remban(&command("3", vec![]), &roster(), &BanList::new(), now());
        assert!(report.replies.is_empty() && report.lifted.is_empty());
    }

    #[test]
    fn migrated_bans_are_lifted_the_same_way() {
        let mut bans = BanList::new();
        bans.issue(Ban { source: BanSource::Migrated, ..ban_until(None) });
        assert!(bans.is_banned(2, now()));
        assert_eq!(remban(&command("2", vec![]), &roster(), &bans, now()).lifted, vec![2]);
    }
}
