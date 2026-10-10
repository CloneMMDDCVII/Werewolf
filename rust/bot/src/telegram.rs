//! The bot's view of Telegram: what comes in ([`Incoming`]) and the few
//! calls that go out ([`Telegram`]). Everything above this module is
//! Telegram-library-free, so whole games run in tests against
//! [`crate::fake::FakeTelegram`]; `teloxide_adapter` is the one real
//! implementation.

use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MsgRef {
    pub chat: i64,
    pub id: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Button {
    /// Sends `data` back as a callback query when pressed (max 64 bytes).
    Callback { text: String, data: String },
    /// Opens a link, e.g. `https://t.me/<bot>?start=...` to start the bot
    /// privately with a payload.
    Url { text: String, url: String },
}

impl Button {
    pub fn text(&self) -> &str {
        match self {
            Button::Callback { text, .. } | Button::Url { text, .. } => text,
        }
    }
}

/// Inline keyboard: rows of buttons.
pub type Keyboard = Vec<Vec<Button>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// The user blocked the bot or never started it: private messages
    /// can't be delivered.
    Unreachable,
    /// Telegram's flood control: wait this many seconds and try again.
    RetryAfter(u64),
    Other(String),
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendError::Unreachable => write!(f, "user can't be messaged"),
            SendError::RetryAfter(s) => write!(f, "rate limited for {s}s"),
            SendError::Other(e) => write!(f, "{e}"),
        }
    }
}

/// Outgoing calls. Text is Telegram HTML.
#[async_trait]
pub trait Telegram: Send + Sync {
    async fn send(&self, chat: i64, text: &str, keyboard: Option<Keyboard>) -> Result<MsgRef, SendError>;
    /// Replaces a message's text; `keyboard: None` removes its buttons.
    async fn edit(&self, msg: MsgRef, text: &str, keyboard: Option<Keyboard>) -> Result<(), SendError>;
    /// Sends a file (the `/getlang` language files).
    async fn send_document(&self, chat: i64, file_name: &str, bytes: Vec<u8>) -> Result<MsgRef, SendError>;
    /// Removes a message's buttons, keeping its text.
    async fn clear_keyboard(&self, msg: MsgRef) -> Result<(), SendError>;
    /// Stops the button's loading spinner, optionally with a short toast.
    async fn answer_callback(&self, callback_id: &str, text: Option<&str>) -> Result<(), SendError>;
    async fn is_group_admin(&self, chat: i64, user: i64) -> Result<bool, SendError>;
    /// Sends `text` as a reply to message `reply_to` in the same chat.
    async fn send_reply(&self, chat: i64, text: &str, reply_to: i32) -> Result<MsgRef, SendError>;
    /// In the group and allowed to post: not left, kicked or muted
    /// (GeneralCommands.cs:334-345).
    async fn is_group_member(&self, chat: i64, user: i64) -> Result<bool, SendError>;
    /// The contents of a file someone sent, by its Telegram file id.
    async fn download(&self, file_id: &str) -> Result<Vec<u8>, SendError>;
    /// The bot's @username, for `t.me` links.
    fn bot_username(&self) -> &str;
}

/// Waits out Telegram's flood control and resends, up to a few times. The
/// bot's whole output goes through this, so callers never see
/// `RetryAfter`.
pub struct Retrying<T> {
    inner: T,
    max_attempts: u32,
}

impl<T: Telegram> Retrying<T> {
    pub fn new(inner: T) -> Self {
        Retrying { inner, max_attempts: 5 }
    }

    pub fn inner(&self) -> &T {
        &self.inner
    }
}

macro_rules! with_retry {
    ($self:ident, $call:expr) => {{
        let mut attempt = 0;
        loop {
            attempt += 1;
            match $call.await {
                Err(SendError::RetryAfter(secs)) if attempt < $self.max_attempts => {
                    log::warn!("Telegram flood control: waiting {secs}s before retrying");
                    tokio::time::sleep(Duration::from_secs(secs.max(1))).await;
                }
                other => break other,
            }
        }
    }};
}

#[async_trait]
impl<T: Telegram> Telegram for Retrying<T> {
    async fn send(&self, chat: i64, text: &str, keyboard: Option<Keyboard>) -> Result<MsgRef, SendError> {
        with_retry!(self, self.inner.send(chat, text, keyboard.clone()))
    }

    async fn edit(&self, msg: MsgRef, text: &str, keyboard: Option<Keyboard>) -> Result<(), SendError> {
        with_retry!(self, self.inner.edit(msg, text, keyboard.clone()))
    }

    async fn clear_keyboard(&self, msg: MsgRef) -> Result<(), SendError> {
        with_retry!(self, self.inner.clear_keyboard(msg))
    }

    async fn send_document(&self, chat: i64, file_name: &str, bytes: Vec<u8>) -> Result<MsgRef, SendError> {
        with_retry!(self, self.inner.send_document(chat, file_name, bytes.clone()))
    }

    async fn answer_callback(&self, callback_id: &str, text: Option<&str>) -> Result<(), SendError> {
        with_retry!(self, self.inner.answer_callback(callback_id, text))
    }

    async fn is_group_admin(&self, chat: i64, user: i64) -> Result<bool, SendError> {
        with_retry!(self, self.inner.is_group_admin(chat, user))
    }

    async fn send_reply(&self, chat: i64, text: &str, reply_to: i32) -> Result<MsgRef, SendError> {
        with_retry!(self, self.inner.send_reply(chat, text, reply_to))
    }

    async fn is_group_member(&self, chat: i64, user: i64) -> Result<bool, SendError> {
        with_retry!(self, self.inner.is_group_member(chat, user))
    }

    async fn download(&self, file_id: &str) -> Result<Vec<u8>, SendError> {
        with_retry!(self, self.inner.download(file_id))
    }

    fn bot_username(&self) -> &str {
        self.inner.bot_username()
    }
}

#[async_trait]
impl<T: Telegram + ?Sized> Telegram for Arc<T> {
    async fn send(&self, chat: i64, text: &str, keyboard: Option<Keyboard>) -> Result<MsgRef, SendError> {
        (**self).send(chat, text, keyboard).await
    }
    async fn edit(&self, msg: MsgRef, text: &str, keyboard: Option<Keyboard>) -> Result<(), SendError> {
        (**self).edit(msg, text, keyboard).await
    }
    async fn clear_keyboard(&self, msg: MsgRef) -> Result<(), SendError> {
        (**self).clear_keyboard(msg).await
    }
    async fn send_document(&self, chat: i64, file_name: &str, bytes: Vec<u8>) -> Result<MsgRef, SendError> {
        (**self).send_document(chat, file_name, bytes).await
    }
    async fn answer_callback(&self, callback_id: &str, text: Option<&str>) -> Result<(), SendError> {
        (**self).answer_callback(callback_id, text).await
    }
    async fn is_group_admin(&self, chat: i64, user: i64) -> Result<bool, SendError> {
        (**self).is_group_admin(chat, user).await
    }
    async fn send_reply(&self, chat: i64, text: &str, reply_to: i32) -> Result<MsgRef, SendError> {
        (**self).send_reply(chat, text, reply_to).await
    }
    async fn is_group_member(&self, chat: i64, user: i64) -> Result<bool, SendError> {
        (**self).is_group_member(chat, user).await
    }
    async fn download(&self, file_id: &str) -> Result<Vec<u8>, SendError> {
        (**self).download(file_id).await
    }
    fn bot_username(&self) -> &str {
        (**self).bot_username()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: i64,
    /// First and last name, as the legacy `GetName()` showed it.
    pub name: String,
    pub username: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatKind {
    Private,
    Group { title: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chat {
    pub id: i64,
    pub kind: ChatKind,
}

impl Chat {
    pub fn is_group(&self) -> bool {
        matches!(self.kind, ChatKind::Group { .. })
    }

    pub fn title(&self) -> &str {
        match &self.kind {
            ChatKind::Group { title } => title,
            ChatKind::Private => "",
        }
    }
}

/// A command message: `/name args`, with any `@botname` suffix already
/// stripped and the name lowercased, plus what the permission gate and the
/// moderation commands need from the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub chat: Chat,
    pub from: User,
    pub name: String,
    pub args: String,
    /// A person, an admin posting anonymously as the group, or a channel.
    pub sender: moderation::Sender,
    pub reply_to: Option<moderation::ReplyTo>,
    pub mentions: Vec<moderation::Mention>,
    /// The file the command replies to, for `/uploadlang`.
    pub replied_document: Option<Document>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub file_id: String,
    pub file_name: Option<String>,
}

impl Command {
    /// A plain command from a person, as tests and most messages have.
    pub fn new(chat: Chat, from: User, name: &str, args: &str) -> Self {
        Command {
            chat,
            from,
            name: name.to_lowercase(),
            args: args.to_string(),
            sender: moderation::Sender::User,
            reply_to: None,
            mentions: vec![],
            replied_document: None,
        }
    }
}

/// What the bot reacts to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    Command(Command),
    /// A press on one of the bot's inline buttons.
    Callback { id: String, from: User, message: Option<MsgRef>, data: String },
}

/// Splits `/Start@WerewolfBot join-100` into `("start", "join-100")`.
/// Returns `None` for text that isn't a command, or a command addressed to
/// a different bot.
pub fn parse_command(text: &str, bot_username: &str) -> Option<(String, String)> {
    let text = text.trim();
    let rest = text.strip_prefix('/')?;
    let (head, args) = match rest.split_once(char::is_whitespace) {
        Some((h, a)) => (h, a.trim()),
        None => (rest, ""),
    };
    let (name, target) = match head.split_once('@') {
        Some((n, t)) => (n, Some(t)),
        None => (head, None),
    };
    if let Some(t) = target {
        if !t.eq_ignore_ascii_case(bot_username) {
            return None;
        }
    }
    if name.is_empty() {
        return None;
    }
    Some((name.to_lowercase(), args.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_parse() {
        assert_eq!(parse_command("/startgame", "wolfbot"), Some(("startgame".into(), "".into())));
        assert_eq!(parse_command("/Start@WolfBot join-100", "wolfbot"), Some(("start".into(), "join-100".into())));
        assert_eq!(parse_command("/extend@otherbot 30", "wolfbot"), None);
        assert_eq!(parse_command("hello", "wolfbot"), None);
        assert_eq!(parse_command("/extend   30 ", "wolfbot"), Some(("extend".into(), "30".into())));
    }

    struct Flaky {
        failures: std::sync::Mutex<u32>,
    }

    #[async_trait]
    impl Telegram for Flaky {
        async fn send(&self, chat: i64, _: &str, _: Option<Keyboard>) -> Result<MsgRef, SendError> {
            let mut f = self.failures.lock().unwrap();
            if *f > 0 {
                *f -= 1;
                return Err(SendError::RetryAfter(5));
            }
            Ok(MsgRef { chat, id: 1 })
        }
        async fn edit(&self, _: MsgRef, _: &str, _: Option<Keyboard>) -> Result<(), SendError> {
            Ok(())
        }
        async fn clear_keyboard(&self, _: MsgRef) -> Result<(), SendError> {
            Ok(())
        }
        async fn send_document(&self, chat: i64, _: &str, _: Vec<u8>) -> Result<MsgRef, SendError> {
            Ok(MsgRef { chat, id: 1 })
        }
        async fn answer_callback(&self, _: &str, _: Option<&str>) -> Result<(), SendError> {
            Ok(())
        }
        async fn send_reply(&self, chat: i64, _: &str, _: i32) -> Result<MsgRef, SendError> {
            Ok(MsgRef { chat, id: 1 })
        }
        async fn is_group_member(&self, _: i64, _: i64) -> Result<bool, SendError> {
            Ok(true)
        }
        async fn download(&self, _: &str) -> Result<Vec<u8>, SendError> {
            Ok(vec![])
        }
        async fn is_group_admin(&self, _: i64, _: i64) -> Result<bool, SendError> {
            Ok(false)
        }
        fn bot_username(&self) -> &str {
            "flaky"
        }
    }

    #[tokio::test(start_paused = true)]
    async fn rate_limits_are_waited_out() {
        let tg = Retrying::new(Flaky { failures: std::sync::Mutex::new(1) });
        let start = tokio::time::Instant::now();
        assert!(tg.send(1, "hi", None).await.is_ok());
        assert_eq!(start.elapsed(), Duration::from_secs(5));
    }

    #[tokio::test(start_paused = true)]
    async fn retries_give_up_eventually() {
        let tg = Retrying::new(Flaky { failures: std::sync::Mutex::new(100) });
        assert_eq!(tg.send(1, "hi", None).await, Err(SendError::RetryAfter(5)));
    }
}
