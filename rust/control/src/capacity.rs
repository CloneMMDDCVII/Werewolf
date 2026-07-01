//! Dynamic admission control: capacity is derived from actual available
//! system memory rather than a hardcoded constant, so the same binary
//! self-calibrates whether it's running on a Raspberry Pi or a beefy
//! production box.
//!
//! Re-checked lazily — at most once per `min_recheck_interval` — and only
//! when a new game is actually being requested. This is a single-purpose
//! server with no other regular workload, so there's no value in polling
//! on a timer when nobody's starting games; on-demand + a floor interval
//! is enough.

use std::time::{Duration, Instant};
use sysinfo::System;

/// Reserved headroom the orchestrator will never allocate into, so the OS
/// and any co-located process always has room to breathe. Whichever is
/// larger of the two applies.
const REDLINE_PERCENT: f64 = 0.20;
const REDLINE_FLOOR_BYTES: u64 = 512 * 1024 * 1024;

/// How rarely we're willing to re-read system memory, even under a burst
/// of game-start requests.
const MIN_RECHECK_INTERVAL: Duration = Duration::from_secs(120);

pub struct CapacityMonitor {
    sys: System,
    per_game_estimate_bytes: u64,
    last_checked: Option<Instant>,
    cached_capacity: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AdmissionDecision {
    Admit,
    Reject { current: usize, capacity: usize },
}

impl CapacityMonitor {
    /// `per_game_estimate_bytes` should already include a generous safety
    /// margin over measured per-game RSS growth (e.g. 2-3x) — better to
    /// undercount capacity than overshoot it.
    pub fn new(per_game_estimate_bytes: u64) -> Self {
        CapacityMonitor {
            sys: System::new(),
            per_game_estimate_bytes,
            last_checked: None,
            cached_capacity: 0,
        }
    }

    /// Call this when a new game is requested. Recomputes capacity from
    /// live system memory if enough time has passed since the last check,
    /// otherwise reuses the cached value.
    pub fn try_admit(&mut self, active_games: usize) -> AdmissionDecision {
        let should_recheck = match self.last_checked {
            None => true,
            Some(t) => t.elapsed() >= MIN_RECHECK_INTERVAL,
        };

        if should_recheck {
            self.recompute_capacity();
        }

        if active_games < self.cached_capacity {
            AdmissionDecision::Admit
        } else {
            AdmissionDecision::Reject {
                current: active_games,
                capacity: self.cached_capacity,
            }
        }
    }

    fn recompute_capacity(&mut self) {
        self.sys.refresh_memory();
        let total = self.sys.total_memory();
        let available = self.sys.available_memory();

        let redline = (total as f64 * REDLINE_PERCENT) as u64;
        let redline = redline.max(REDLINE_FLOOR_BYTES);

        let usable = available.saturating_sub(redline);
        self.cached_capacity = (usable / self.per_game_estimate_bytes.max(1)) as usize;
        self.last_checked = Some(Instant::now());
    }
}
