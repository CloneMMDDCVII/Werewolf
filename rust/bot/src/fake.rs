//! An in-memory Telegram for tests and local dry runs: records everything
//! the bot sends and lets a test press buttons. No network, no token.

use crate::telegram::{Button, Keyboard, MsgRef, SendError, Telegram};
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeMessage {
    pub msg: MsgRef,
    pub text: String,
    pub keyboard: Option<Keyboard>,
    /// How many times it was edited after being sent.
    pub edits: u32,
    /// The message this one replies to.
    pub reply_to: Option<i32>,
}

#[derive(Default)]
struct State {
    next_id: i32,
    /// Every message, in send order, as it looks now (edits applied).
    messages: Vec<FakeMessage>,
    callback_answers: Vec<(String, Option<String>)>,
    admins: HashSet<(i64, i64)>,
    /// (group, user) pairs where the user has left the group.
    left: HashSet<(i64, i64)>,
    files: HashMap<String, Vec<u8>>,
    unreachable: HashSet<i64>,
    /// Chats whose next send fails with RetryAfter(secs).
    flood: HashMap<i64, u64>,
    send_attempts: u32,
    /// Chats whose next send panics.
    panic_on: HashSet<i64>,
}

pub struct FakeTelegram {
    username: String,
    state: Mutex<State>,
}

impl Default for FakeTelegram {
    fn default() -> Self {
        Self::new("werewolf_test_bot")
    }
}

impl FakeTelegram {
    pub fn new(username: &str) -> Self {
        FakeTelegram {
            username: username.into(),
            state: Mutex::new(State {
                next_id: 1,
                ..Default::default()
            }),
        }
    }

    pub fn make_admin(&self, chat: i64, user: i64) {
        self.state.lock().unwrap().admins.insert((chat, user));
    }

    /// The user isn't (or is no longer) a member of the group.
    pub fn leave_group(&self, chat: i64, user: i64) {
        self.state.lock().unwrap().left.insert((chat, user));
    }

    /// A file users can send the bot; returns its file id.
    pub fn add_file(&self, bytes: Vec<u8>) -> String {
        let mut s = self.state.lock().unwrap();
        let id = format!("file{}", s.files.len() + 1);
        s.files.insert(id.clone(), bytes);
        id
    }

    /// The user has blocked the bot: private messages to them fail.
    pub fn block(&self, user: i64) {
        self.state.lock().unwrap().unreachable.insert(user);
    }

    /// The next send to `chat` is refused with "retry after `secs`".
    pub fn flood_once(&self, chat: i64, secs: u64) {
        self.state.lock().unwrap().flood.insert(chat, secs);
    }

    /// The next send to `chat` panics, as a bug in the game would.
    pub fn panic_once(&self, chat: i64) {
        self.state.lock().unwrap().panic_on.insert(chat);
    }

    pub fn send_attempts(&self) -> u32 {
        self.state.lock().unwrap().send_attempts
    }

    pub fn messages(&self) -> Vec<FakeMessage> {
        self.state.lock().unwrap().messages.clone()
    }

    /// Texts sent to `chat`, in order, as they read now.
    pub fn texts(&self, chat: i64) -> Vec<String> {
        self.messages()
            .into_iter()
            .filter(|m| m.msg.chat == chat)
            .map(|m| m.text)
            .collect()
    }

    /// Everything sent to `chat`, joined, for `contains` checks.
    pub fn transcript(&self, chat: i64) -> String {
        self.texts(chat).join("\n")
    }

    pub fn last(&self, chat: i64) -> Option<FakeMessage> {
        self.messages().into_iter().rev().find(|m| m.msg.chat == chat)
    }

    /// The newest message in `chat` that still has a button labelled
    /// `label`, and that button.
    pub fn find_button(&self, chat: i64, label: &str) -> Option<(MsgRef, Button)> {
        self.messages()
            .into_iter()
            .rev()
            .filter(|m| m.msg.chat == chat)
            .find_map(|m| {
                m.keyboard
                    .iter()
                    .flatten()
                    .flatten()
                    .find(|b| b.text() == label)
                    .map(|b| (m.msg, b.clone()))
            })
    }

    /// Messages in `chat` that still have buttons.
    pub fn open_keyboards(&self, chat: i64) -> Vec<FakeMessage> {
        self.messages()
            .into_iter()
            .filter(|m| m.msg.chat == chat && m.keyboard.is_some())
            .collect()
    }

    pub fn callback_answers(&self) -> Vec<(String, Option<String>)> {
        self.state.lock().unwrap().callback_answers.clone()
    }
}

#[async_trait]
impl Telegram for FakeTelegram {
    async fn send(&self, chat: i64, text: &str, keyboard: Option<Keyboard>) -> Result<MsgRef, SendError> {
        let mut s = self.state.lock().unwrap();
        s.send_attempts += 1;
        if s.panic_on.remove(&chat) {
            drop(s);
            panic!("injected panic sending to {chat}");
        }
        if let Some(secs) = s.flood.remove(&chat) {
            return Err(SendError::RetryAfter(secs));
        }
        if chat > 0 && s.unreachable.contains(&chat) {
            return Err(SendError::Unreachable);
        }
        let msg = MsgRef { chat, id: s.next_id };
        s.next_id += 1;
        s.messages.push(FakeMessage {
            msg,
            text: text.into(),
            keyboard,
            edits: 0,
            reply_to: None,
        });
        Ok(msg)
    }

    async fn edit(&self, msg: MsgRef, text: &str, keyboard: Option<Keyboard>) -> Result<(), SendError> {
        let mut s = self.state.lock().unwrap();
        let m = s
            .messages
            .iter_mut()
            .find(|m| m.msg == msg)
            .ok_or(SendError::Other("no such message".into()))?;
        m.text = text.into();
        m.keyboard = keyboard;
        m.edits += 1;
        Ok(())
    }

    async fn clear_keyboard(&self, msg: MsgRef) -> Result<(), SendError> {
        let mut s = self.state.lock().unwrap();
        if let Some(m) = s.messages.iter_mut().find(|m| m.msg == msg) {
            m.keyboard = None;
        }
        Ok(())
    }

    async fn send_document(&self, chat: i64, file_name: &str, bytes: Vec<u8>) -> Result<MsgRef, SendError> {
        self.send(chat, &format!("[file {file_name}, {} bytes]", bytes.len()), None)
            .await
    }

    async fn answer_callback(&self, callback_id: &str, text: Option<&str>) -> Result<(), SendError> {
        self.state
            .lock()
            .unwrap()
            .callback_answers
            .push((callback_id.into(), text.map(str::to_string)));
        Ok(())
    }

    async fn is_group_admin(&self, chat: i64, user: i64) -> Result<bool, SendError> {
        Ok(self.state.lock().unwrap().admins.contains(&(chat, user)))
    }

    async fn send_reply(&self, chat: i64, text: &str, reply_to: i32) -> Result<MsgRef, SendError> {
        let msg = self.send(chat, text, None).await?;
        let mut s = self.state.lock().unwrap();
        if let Some(m) = s.messages.iter_mut().find(|m| m.msg == msg) {
            m.reply_to = Some(reply_to);
        }
        Ok(msg)
    }

    async fn is_group_member(&self, chat: i64, user: i64) -> Result<bool, SendError> {
        Ok(!self.state.lock().unwrap().left.contains(&(chat, user)))
    }

    async fn download(&self, file_id: &str) -> Result<Vec<u8>, SendError> {
        self.state
            .lock()
            .unwrap()
            .files
            .get(file_id)
            .cloned()
            .ok_or(SendError::Other("no such file".into()))
    }

    fn bot_username(&self) -> &str {
        &self.username
    }
}
