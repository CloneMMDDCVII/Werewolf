//! What every menu in `control` hands a presenter: [`Screen`]s of locale
//! [`Message`]s and [`Button`]s, [`Notice`]s for button presses, and one
//! callback-data codec ([`Action::encode`]/[`Action::decode`]) covering
//! every kind of button. Nothing here knows about Telegram; a presenter
//! resolves locale keys in the viewer's language and draws the keyboard.

use crate::group_config::ConfigAction;
use crate::language_picker::LanguageAction;

/// A piece of text a presenter resolves in the viewer's language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Text {
    Locale(String),
    Raw(String),
}

impl Text {
    pub fn key(key: &str) -> Self {
        Text::Locale(key.to_string())
    }
}

/// A locale string and the arguments substituted into its `{0}`, `{1}`...
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub key: String,
    pub args: Vec<Text>,
}

impl Message {
    pub fn new(key: &str) -> Self {
        Message { key: key.to_string(), args: vec![] }
    }

    pub fn with(key: &str, args: Vec<Text>) -> Self {
        Message { key: key.to_string(), args }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ButtonAction {
    Callback(Action),
    /// A link that opens a private chat with the bot (`RequestPM`'s
    /// "Start Me" button).
    StartBotLink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub label: Text,
    pub action: ButtonAction,
}

/// What a presenter shows: a message and its keyboard, one inner `Vec`
/// per row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    pub message: Message,
    pub rows: Vec<Vec<Button>>,
}

/// A short notice on the button press itself (Telegram's callback answer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub message: Message,
    /// Shown as a dialog rather than a passing toast.
    pub alert: bool,
}

/// The answer to a command that opens a menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutcome {
    /// Send `menu` to the user privately, and `chat_notice` (if any) to
    /// the chat the command came from. If the private message fails
    /// because they never started the bot, send `fallback` to that chat
    /// instead (`AdminCommands.cs:96-99`, `GeneralCommands.cs:147-153`).
    SendPrivately { menu: Screen, chat_notice: Option<Message>, fallback: Screen },
    /// Reply in the chat the command came from with this locale string.
    Refuse(Message),
    /// Reply in that chat with this HTML text. For the few legacy commands
    /// whose replies were hardcoded English rather than locale strings.
    Reply(String),
}

/// The answer to a button press.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallbackOutcome {
    /// Replace the pressed message with `screen`, after showing `notice`.
    Show { screen: Screen, notice: Option<Notice> },
    /// Answer the press with a notice and leave the message alone.
    Notify(Notice),
    /// Nothing to do (unknown group, or an inert button).
    Ignore,
}

/// Who sent a command or pressed a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Actor {
    /// A group admin, a global admin, or a developer: the three roles the
    /// legacy checks accept (`UpdateHandler.cs:1031`).
    pub can_configure: bool,
}

/// Any button's callback, as one value a presenter can encode into
/// Telegram callback data and decode back on a press.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Config(ConfigAction),
    Language(LanguageAction),
}

impl Action {
    /// Telegram allows 64 bytes of callback data; every action this crate
    /// builds stays within it (checked by tests against the real
    /// language files and the longest group ids).
    pub fn encode(&self) -> String {
        match self {
            Action::Config(a) => a.encode(),
            Action::Language(a) => a.encode(),
        }
    }

    pub fn decode(data: &str) -> Result<Self, DecodeError> {
        match data.split('|').next() {
            Some("cfg") => ConfigAction::decode(data).map(Action::Config),
            Some("lng") => LanguageAction::decode(data).map(Action::Language),
            _ => Err(DecodeError(data.to_string())),
        }
    }
}

/// Why a callback string didn't decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError(pub String);

/// `Helpers.cs::RequestPM`.
pub fn start_me_prompt() -> Screen {
    Screen {
        message: Message::new("StartMe"),
        rows: vec![vec![Button { label: Text::Raw("Start Me".to_string()), action: ButtonAction::StartBotLink }]],
    }
}

/// Pairs buttons into rows of two, the last one alone if odd
/// (`GetConfigMenu`/`GetConfigSubmenu`'s `twoMenu`).
pub fn rows_of(buttons: Vec<Button>, width: usize) -> Vec<Vec<Button>> {
    buttons.chunks(width).map(|c| c.to_vec()).collect()
}
