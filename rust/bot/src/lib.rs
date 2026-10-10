//! Werewolf for Telegram: the bot itself. Wires the engine
//! (`game-engine`), the lobby (`control`), storage (`db`), moderation and
//! the language files to Telegram.
//!
//! - [`telegram`]: the bot's view of Telegram (the `Telegram` trait and
//!   incoming updates); [`teloxide_adapter`] is the real implementation and
//!   [`fake`] an in-memory one for tests.
//! - [`core`]: command dispatch, the permission gate, each group's lobby.
//! - [`game`]: one running game and its Telegram presenter.
//! - [`health`]: the optional `/healthz` endpoint.
//! - [`config`], [`menus`], [`text`], [`join_timer`], [`spawn`], [`sync`].

pub mod config;
pub mod core;
pub mod fake;
pub mod game;
pub mod health;
pub mod join_timer;
pub mod lang_admin;
pub mod menus;
pub mod spawn;
pub mod sync;
pub mod telegram;
pub mod teloxide_adapter;
pub mod text;
