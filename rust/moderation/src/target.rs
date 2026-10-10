//! Working out which player an admin command is about, from a reply,
//! @mentions, text mentions or a typed id. Telegram parsing stays with
//! the presenter: it hands over a [`CommandMessage`] with the entities
//! already pulled out.

/// What the bot knows about a player (the legacy `Player` row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRecord {
    pub telegram_id: i64,
    pub name: String,
    pub username: Option<String>,
}

/// Looks players up in whatever storage the bot runs on.
pub trait PlayerDirectory {
    fn by_id(&self, telegram_id: i64) -> Option<PlayerRecord>;
    /// Without the leading `@`, case-insensitive (SQL Server's default
    /// collation made the C# lookups case-insensitive).
    fn by_username(&self, username: &str) -> Option<PlayerRecord>;
}

/// One mention entity in the command's text, in message order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mention {
    /// `@username` (`MessageEntityType.Mention`), without the `@`.
    Username(String),
    /// A mention of a user without a username (`TextMention`), which
    /// carries the user itself.
    User(PlayerRecord),
}

/// The message a reply points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplyTo {
    pub from: i64,
    pub forwarded_from: Option<i64>,
}

/// An admin command, already parsed out of its Telegram message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandMessage {
    pub sender_id: i64,
    pub reply_to: Option<ReplyTo>,
    pub mentions: Vec<Mention>,
    /// Everything after the command word (the C# `args[1]`).
    pub argument: Option<String>,
}

impl CommandMessage {
    pub(crate) fn argument_words(&self) -> impl Iterator<Item = &str> {
        self.argument.as_deref().unwrap_or("").split_whitespace()
    }

    pub(crate) fn numeric_ids(&self) -> impl Iterator<Item = i64> + '_ {
        self.argument_words().filter_map(|w| w.parse::<i64>().ok())
    }
}

/// `Update.GetTarget` (Extensions.cs:136-185), used by `/getban`: the
/// player replied to (or the original author of a forwarded message), else
/// the first `@mention`, else the first text mention, else the id or
/// username typed as the first word. Anything that doesn't resolve falls
/// back to the sender, a legacy quirk kept for parity: `/getban` with no
/// target reports on yourself. `None` only when the sender isn't a known
/// player either.
pub fn resolve_target(message: &CommandMessage, players: &dyn PlayerDirectory) -> Option<PlayerRecord> {
    let sender = || players.by_id(message.sender_id);

    if let Some(reply) = message.reply_to {
        return players.by_id(reply.forwarded_from.unwrap_or(reply.from)).or_else(sender);
    }

    let Some(first_word) = message.argument_words().next() else {
        return sender();
    };

    let username_mention = message.mentions.iter().find_map(|m| match m {
        Mention::Username(name) => Some(name.as_str()),
        Mention::User(_) => None,
    });
    let user_mention = message.mentions.iter().find_map(|m| match m {
        Mention::User(user) => Some(user.telegram_id),
        Mention::Username(_) => None,
    });

    let found = if let Some(username) = username_mention {
        players.by_username(username)
    } else if let Some(id) = user_mention {
        players.by_id(id)
    } else if let Ok(id) = first_word.parse::<i64>() {
        players.by_id(id)
    } else {
        players.by_username(first_word.trim_start_matches('@'))
    };
    found.or_else(sender)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A fixed set of players for tests across the crate.
    pub(crate) struct Players(pub Vec<PlayerRecord>);

    impl PlayerDirectory for Players {
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

    pub(crate) fn player(id: i64, name: &str, username: Option<&str>) -> PlayerRecord {
        PlayerRecord { telegram_id: id, name: name.into(), username: username.map(Into::into) }
    }

    pub(crate) fn roster() -> Players {
        Players(vec![player(1, "Alice", Some("alice")), player(2, "Paul", Some("paul")), player(3, "Rita", None)])
    }

    fn from_alice() -> CommandMessage {
        CommandMessage { sender_id: 1, ..Default::default() }
    }

    fn target(message: CommandMessage) -> i64 {
        resolve_target(&message, &roster()).unwrap().telegram_id
    }

    #[test]
    fn a_reply_targets_its_author() {
        let reply_to = Some(ReplyTo { from: 2, forwarded_from: None });
        assert_eq!(target(CommandMessage { reply_to, ..from_alice() }), 2);
    }

    #[test]
    fn a_reply_to_a_forward_targets_the_original_author() {
        let reply_to = Some(ReplyTo { from: 3, forwarded_from: Some(2) });
        assert_eq!(target(CommandMessage { reply_to, ..from_alice() }), 2);
    }

    #[test]
    fn a_username_mention_is_case_insensitive() {
        let message = CommandMessage {
            mentions: vec![Mention::Username("PAUL".into())],
            argument: Some("@PAUL".into()),
            ..from_alice()
        };
        assert_eq!(target(message), 2);
    }

    #[test]
    fn a_text_mention_targets_that_user() {
        let message = CommandMessage {
            mentions: vec![Mention::User(player(3, "Rita", None))],
            argument: Some("Rita".into()),
            ..from_alice()
        };
        assert_eq!(target(message), 3);
    }

    #[test]
    fn a_numeric_id_targets_that_player() {
        assert_eq!(target(CommandMessage { argument: Some("2 extra words".into()), ..from_alice() }), 2);
    }

    #[test]
    fn a_bare_username_without_an_entity_still_resolves() {
        assert_eq!(target(CommandMessage { argument: Some("paul".into()), ..from_alice() }), 2);
    }

    #[test]
    fn no_argument_falls_back_to_the_sender() {
        assert_eq!(target(from_alice()), 1);
        assert_eq!(target(CommandMessage { argument: Some("   ".into()), ..from_alice() }), 1);
    }

    #[test]
    fn an_unknown_target_falls_back_to_the_sender() {
        assert_eq!(target(CommandMessage { argument: Some("nobody".into()), ..from_alice() }), 1);
        let reply_to = Some(ReplyTo { from: 99, forwarded_from: None });
        assert_eq!(target(CommandMessage { reply_to, ..from_alice() }), 1);
    }

    #[test]
    fn an_unknown_sender_with_no_target_resolves_to_nobody() {
        let message = CommandMessage { sender_id: 42, ..Default::default() };
        assert_eq!(resolve_target(&message, &roster()), None);
    }
}
