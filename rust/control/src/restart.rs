//! Two restart paths, both triggerable via an admin Telegram command:
//!
//! - **Graceful**: stop admitting new games, wait for in-flight games to
//!   finish, then exit. `systemd` (`Restart=always`) brings the process
//!   back up. No orphans possible since games are tasks in this process,
//!   not separate OS processes.
//! - **Force**: exit immediately, abandoning in-flight games. For
//!   emergencies where waiting isn't acceptable.

use std::future::Future;
use tokio::sync::watch;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartMode {
    Graceful,
    Force,
}

/// Signals game-admission code to stop accepting new games, and lets
/// callers wait for in-flight games to drain before the process exits.
pub struct ShutdownController {
    accepting: watch::Sender<bool>,
}

impl ShutdownController {
    pub fn new() -> (Self, watch::Receiver<bool>) {
        let (tx, rx) = watch::channel(true);
        (ShutdownController { accepting: tx }, rx)
    }

    pub fn is_accepting(&self) -> bool {
        *self.accepting.borrow()
    }

    /// Begin graceful shutdown: stop admitting, then wait for every
    /// in-flight game task to finish before returning.
    pub async fn graceful_shutdown(&self, in_flight: Vec<JoinHandle<()>>) {
        let _ = self.accepting.send(false);
        for handle in in_flight {
            let _ = handle.await;
        }
    }

    /// Force shutdown: stop admitting and return immediately without
    /// waiting on in-flight games. Callers should follow this with an
    /// actual process exit — abandoned tasks keep running otherwise,
    /// since dropping a `JoinHandle` does not cancel its task.
    pub fn force_shutdown_signal(&self) {
        let _ = self.accepting.send(false);
    }
}

/// Runs a future, but if `mode` is `Force`, doesn't wait for it —
/// fire-and-forget so an admin's "force restart now" isn't blocked by
/// slow-draining game tasks.
pub async fn shutdown(mode: RestartMode, drain: impl Future<Output = ()>) {
    match mode {
        RestartMode::Graceful => drain.await,
        RestartMode::Force => {
            // Intentionally not awaited: caller exits right after this.
        }
    }
}
