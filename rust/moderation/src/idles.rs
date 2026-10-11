//! `/getidles` (AdminCommands.cs:220-271): how often each target was
//! killed for not voting in the last 24 hours, overall and in this group.

use crate::target::{CommandMessage, Mention};
use i18n::LanguagePack;

/// The C# counted kills newer than `DATEADD(day, -1, GETDATE())`.
pub const IDLE_WINDOW_SECS: i64 = 86_400;

/// Who `/getidles` reports on, in the C#'s order: each numeric word of the
/// argument, then each text mention, then the author of the message
/// replied to. `@username` mentions are not looked up (the C# didn't).
pub fn getidles_targets(message: &CommandMessage) -> Vec<i64> {
    let mut ids: Vec<i64> = message.numeric_ids().collect();
    ids.extend(message.mentions.iter().filter_map(|m| match m {
        Mention::User(user) => Some(user.telegram_id),
        Mention::Username(_) => None,
    }));
    ids.extend(message.reply_to.map(|r| r.from));
    ids
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdleCount {
    pub telegram_id: i64,
    /// The player's name, if the bot knows them (the C# asked Telegram
    /// for their first name and printed nothing when that failed).
    pub name: Option<String>,
    pub total: i64,
    pub in_group: i64,
}

/// One line per target: the `IdleCount` sentence, then `GroupIdleCount`.
pub fn getidles_report(counts: &[IdleCount], pack: &LanguagePack) -> String {
    counts
        .iter()
        .map(|c| {
            let who = format!("{} ({})", c.telegram_id, c.name.as_deref().unwrap_or(""));
            let total = pack
                .format("IdleCount", &[&who, &c.total.to_string()])
                .unwrap_or_default();
            let group = pack
                .format("GroupIdleCount", &[&c.in_group.to_string()])
                .unwrap_or_default();
            format!("{total} {group}\n")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::tests::player;
    use crate::target::ReplyTo;

    #[test]
    fn targets_come_from_ids_then_text_mentions_then_the_reply() {
        let message = CommandMessage {
            sender_id: 1,
            reply_to: Some(ReplyTo {
                from: 9,
                forwarded_from: None,
            }),
            mentions: vec![Mention::Username("paul".into()), Mention::User(player(7, "Rita", None))],
            argument: Some("2 @paul Rita 3".into()),
        };
        assert_eq!(getidles_targets(&message), vec![2, 3, 7, 9]);
    }

    #[test]
    fn no_targets_means_an_empty_report() {
        assert!(getidles_targets(&CommandMessage::default()).is_empty());
    }
}
