//! Locking that outlives a panic. A game that panics while holding one of
//! the bot's shared locks must not turn every later command for every
//! group into a panic too, so a poisoned lock is used as it is: each
//! critical section touches one group's entry, and a half-updated entry
//! is better than a bot that can no longer start games anywhere.

use std::sync::{Mutex, MutexGuard, PoisonError};

pub trait LockExt<T> {
    /// `lock()`, ignoring poisoning.
    fn locked(&self) -> MutexGuard<'_, T>;
}

impl<T> LockExt<T> for Mutex<T> {
    fn locked(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
