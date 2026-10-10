//! The real Telegram: [`Telegram`] on a teloxide `Bot`, and long polling
//! that turns Telegram updates into [`Incoming`]. This is the only module
//! that knows teloxide exists.

use crate::telegram::{Button, Chat, ChatKind, Command, Incoming, Keyboard, MsgRef, SendError, Telegram, User};
use async_trait::async_trait;
use moderation::{Mention, PlayerRecord, ReplyTo, Sender};
use std::time::Duration;
use teloxide::payloads::{
    AnswerCallbackQuerySetters, EditMessageTextSetters, GetUpdatesSetters, SendMessageSetters,
};
use teloxide::prelude::Requester;
use teloxide::types::{
    AllowedUpdate, ChatId, InlineKeyboardButton, InlineKeyboardMarkup, InputFile, LinkPreviewOptions, Message,
    MessageEntityKind, MessageId, ParseMode, UpdateKind, UserId,
};
use teloxide::{ApiError, Bot, RequestError};

pub struct TeloxideTelegram {
    bot: Bot,
    username: String,
}

/// Why the bot couldn't start talking to Telegram.
#[derive(Debug)]
pub enum ConnectError {
    /// Telegram says the token is wrong or revoked.
    BadToken,
    Network(String),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::BadToken => write!(
                f,
                "Telegram rejected the token. Check it with @BotFather (/mybots, API Token) and try again."
            ),
            ConnectError::Network(e) => write!(f, "could not reach Telegram: {e}"),
        }
    }
}

impl std::error::Error for ConnectError {}

impl TeloxideTelegram {
    /// Checks the token with `getMe` and learns the bot's username.
    pub async fn connect(token: &str) -> Result<Self, ConnectError> {
        let bot = Bot::new(token);
        match bot.get_me().await {
            Ok(me) => Ok(TeloxideTelegram { username: me.username().to_string(), bot }),
            Err(RequestError::Api(ApiError::InvalidToken)) => Err(ConnectError::BadToken),
            Err(e) => Err(ConnectError::Network(e.to_string())),
        }
    }

    pub fn bot(&self) -> &Bot {
        &self.bot
    }
}

fn map_error(e: RequestError) -> SendError {
    match e {
        RequestError::RetryAfter(secs) => SendError::RetryAfter(u64::from(secs.seconds())),
        RequestError::Api(
            ApiError::BotBlocked
            | ApiError::UserDeactivated
            | ApiError::CantInitiateConversation
            | ApiError::CantTalkWithBots
            | ApiError::BotKicked
            | ApiError::BotKickedFromSupergroup
            | ApiError::ChatNotFound,
        ) => SendError::Unreachable,
        other => SendError::Other(other.to_string()),
    }
}

fn markup(keyboard: Keyboard) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(keyboard.into_iter().map(|row| {
        row.into_iter()
            .filter_map(|b| match b {
                Button::Callback { text, data } => Some(InlineKeyboardButton::callback(text, data)),
                Button::Url { text, url } => match url.parse::<url::Url>() {
                    Ok(url) => Some(InlineKeyboardButton::url(text, url)),
                    Err(e) => {
                        log::error!("dropping button {text:?}: bad url {url:?}: {e}");
                        None
                    }
                },
            })
            .collect::<Vec<_>>()
    }))
}

fn no_preview() -> LinkPreviewOptions {
    LinkPreviewOptions { is_disabled: true, url: None, prefer_small_media: false, prefer_large_media: false, show_above_text: false }
}

#[async_trait]
impl Telegram for TeloxideTelegram {
    async fn send(&self, chat: i64, text: &str, keyboard: Option<Keyboard>) -> Result<MsgRef, SendError> {
        let mut req = self.bot.send_message(ChatId(chat), text).parse_mode(ParseMode::Html).link_preview_options(no_preview());
        if let Some(k) = keyboard {
            req = req.reply_markup(markup(k));
        }
        let m = req.await.map_err(map_error)?;
        Ok(MsgRef { chat, id: m.id.0 })
    }

    async fn edit(&self, msg: MsgRef, text: &str, keyboard: Option<Keyboard>) -> Result<(), SendError> {
        let mut req = self
            .bot
            .edit_message_text(ChatId(msg.chat), MessageId(msg.id), text)
            .parse_mode(ParseMode::Html)
            .link_preview_options(no_preview());
        if let Some(k) = keyboard {
            req = req.reply_markup(markup(k));
        }
        match req.await {
            Ok(_) | Err(RequestError::Api(ApiError::MessageNotModified)) => Ok(()),
            Err(e) => Err(map_error(e)),
        }
    }

    async fn send_document(&self, chat: i64, file_name: &str, bytes: Vec<u8>) -> Result<MsgRef, SendError> {
        let file = InputFile::memory(bytes).file_name(file_name.to_string());
        let m = self.bot.send_document(ChatId(chat), file).await.map_err(map_error)?;
        Ok(MsgRef { chat, id: m.id.0 })
    }

    async fn clear_keyboard(&self, msg: MsgRef) -> Result<(), SendError> {
        match self.bot.edit_message_reply_markup(ChatId(msg.chat), MessageId(msg.id)).await {
            Ok(_) | Err(RequestError::Api(ApiError::MessageNotModified)) => Ok(()),
            Err(e) => Err(map_error(e)),
        }
    }

    async fn answer_callback(&self, callback_id: &str, text: Option<&str>) -> Result<(), SendError> {
        let mut req = self.bot.answer_callback_query(teloxide::types::CallbackQueryId(callback_id.to_string()));
        if let Some(t) = text {
            req = req.text(t);
        }
        match req.await {
            // Answering too late (after ~15s) fails harmlessly.
            Ok(_) | Err(RequestError::Api(ApiError::InvalidQueryId)) => Ok(()),
            Err(e) => Err(map_error(e)),
        }
    }

    async fn is_group_admin(&self, chat: i64, user: i64) -> Result<bool, SendError> {
        let member = self.bot.get_chat_member(ChatId(chat), UserId(user as u64)).await.map_err(map_error)?;
        Ok(member.is_privileged())
    }

    fn bot_username(&self) -> &str {
        &self.username
    }
}

fn user(u: &teloxide::types::User) -> User {
    User { id: u.id.0 as i64, name: u.full_name(), username: u.username.clone() }
}

fn chat(c: &teloxide::types::Chat) -> Chat {
    if c.is_private() {
        Chat { id: c.id.0, kind: ChatKind::Private }
    } else {
        Chat { id: c.id.0, kind: ChatKind::Group { title: c.title().unwrap_or_default().to_string() } }
    }
}

/// A text message as a [`Command`], if it is one addressed to this bot.
pub fn command_from_message(m: &Message, bot_username: &str) -> Option<Command> {
    let text = m.text()?;
    let (name, args) = crate::telegram::parse_command(text, bot_username)?;
    let sender = match &m.sender_chat {
        None => Sender::User,
        Some(c) if c.id == m.chat.id => Sender::AnonymousGroupAdmin,
        Some(_) => Sender::Channel,
    };
    // Anonymous admins and channels arrive with a placeholder `from`
    // (@GroupAnonymousBot); the gate decides what they may do.
    let from = m.from.as_ref().map(user).unwrap_or(User { id: 0, name: String::new(), username: None });
    let reply_to = m.reply_to_message().and_then(|r| {
        Some(ReplyTo { from: r.from.as_ref()?.id.0 as i64, forwarded_from: r.forward_from_user().map(|u| u.id.0 as i64) })
    });
    let mentions = m
        .parse_entities()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| match e.kind() {
            MessageEntityKind::Mention => Some(Mention::Username(e.text().trim_start_matches('@').to_string())),
            MessageEntityKind::TextMention { user: u } => Some(Mention::User(PlayerRecord {
                telegram_id: u.id.0 as i64,
                name: u.full_name(),
                username: u.username.clone(),
            })),
            _ => None,
        })
        .filter(|mention| !matches!(mention, Mention::Username(u) if u.eq_ignore_ascii_case(bot_username)))
        .collect();
    Some(Command { chat: chat(&m.chat), from, name, args, sender, reply_to, mentions })
}

pub fn incoming(update: teloxide::types::Update, bot_username: &str) -> Option<Incoming> {
    match update.kind {
        UpdateKind::Message(m) => command_from_message(&m, bot_username).map(Incoming::Command),
        UpdateKind::CallbackQuery(q) => Some(Incoming::Callback {
            id: q.id.0,
            from: user(&q.from),
            message: q.message.as_ref().map(|m| MsgRef { chat: m.chat().id.0, id: m.id().0 }),
            data: q.data?,
        }),
        _ => None,
    }
}

/// Long-polls Telegram until `stop` resolves, handing each update to
/// `handle`. Network errors are logged and retried with backoff; the bot
/// never exits because Telegram was briefly unreachable.
pub async fn poll<F, S>(tg: &TeloxideTelegram, mut handle: F, stop: S)
where
    F: FnMut(Incoming),
    S: std::future::Future<Output = ()>,
{
    tokio::pin!(stop);
    let mut offset: i32 = 0;
    let mut backoff = Duration::from_secs(1);
    loop {
        let request = tg
            .bot
            .get_updates()
            .offset(offset)
            .timeout(30)
            .allowed_updates([AllowedUpdate::Message, AllowedUpdate::CallbackQuery]);
        let result = tokio::select! {
            r = request => r,
            _ = &mut stop => return,
        };
        match result {
            Ok(updates) => {
                backoff = Duration::from_secs(1);
                for u in updates {
                    offset = offset.max(u.id.0 as i32 + 1);
                    if let Some(i) = incoming(u, &tg.username) {
                        handle(i);
                    }
                }
            }
            Err(RequestError::RetryAfter(s)) => tokio::time::sleep(s.duration()).await,
            Err(e) => {
                log::warn!("getUpdates failed: {e}; retrying in {}s", backoff.as_secs());
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {}
                    _ = &mut stop => return,
                }
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
        }
    }
}
