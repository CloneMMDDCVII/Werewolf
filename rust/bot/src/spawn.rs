//! Where game tasks run. The engine's `Presenter` is `?Send`, so a game
//! can't move between threads; each one is pinned to one thread of a
//! small pool. Tests use [`LocalSpawner`] instead, so games run on the
//! test's own runtime and its paused clock fast-forwards every timer.

use std::future::Future;
use std::pin::Pin;
use tokio_util::task::LocalPoolHandle;

pub type GameJob = Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = ()>>> + Send>;

pub trait GameSpawner: Send + Sync {
    fn spawn(&self, job: GameJob);
}

/// One thread per CPU, each running many games.
pub struct PoolSpawner(LocalPoolHandle);

impl PoolSpawner {
    pub fn new() -> Self {
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
        PoolSpawner(LocalPoolHandle::new(threads))
    }
}

impl Default for PoolSpawner {
    fn default() -> Self {
        Self::new()
    }
}

impl GameSpawner for PoolSpawner {
    fn spawn(&self, job: GameJob) {
        drop(self.0.spawn_pinned(job));
    }
}

/// Runs games on a `LocalSet` (tests, the dry-run harness), so they share
/// the test's runtime and its paused clock. Create it inside
/// `LocalSet::run_until`; it can then be called from any task, local or
/// not (the join timer runs as a plain `tokio::spawn` task).
pub struct LocalSpawner(tokio::sync::mpsc::UnboundedSender<GameJob>);

#[allow(clippy::new_without_default)] // must be created inside a LocalSet
impl LocalSpawner {
    pub fn new() -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<GameJob>();
        tokio::task::spawn_local(async move {
            while let Some(job) = rx.recv().await {
                tokio::task::spawn_local(job());
            }
        });
        LocalSpawner(tx)
    }
}

impl GameSpawner for LocalSpawner {
    fn spawn(&self, job: GameJob) {
        let _ = self.0.send(job);
    }
}
