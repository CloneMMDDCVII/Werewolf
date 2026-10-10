//! Telegram bot orchestrator: command dispatch, per-group join-phase
//! lifecycle (see `lobby`), dynamic admission control, and graceful/force
//! restart. Games run as tasks inside this single process (see
//! `capacity` module docs) — no separate node processes, so there is no
//! orphan-process problem to solve: if this process dies, every game
//! task dies with it.

pub mod capacity;
pub mod group_config;
pub mod language_picker;
pub mod lobby;
pub mod menu;
pub mod restart;

/// Seam for a future payment provider (Telegram Stars), so donations can be
/// added later without restructuring command dispatch. Not implemented —
/// donations are explicitly out of scope right now.
pub trait PaymentProvider {
    fn handle_payment_update(&self, update_json: &str) -> Result<(), String>;
}
