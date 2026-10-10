//! The join countdown, ported from `Werewolf.cs::GameTimer` (425-500) as a
//! pure state machine: the caller ticks it once a second and sends
//! whatever it returns. `control::lobby::Lobby` decides who may join; this
//! decides when joining ends.

/// Settings.cs:136-137.
pub const GAME_JOIN_TIME: i64 = 180;
pub const MAX_JOIN_TIME: i64 = 300;
/// Werewolf.cs:461: "notify when time is running out".
const IMPORTANT_SECONDS: [i64; 3] = [10, 30, 60];
/// Werewolf.cs:455: who joined is announced in batches this often.
const ANNOUNCE_EVERY: i64 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimerEvent {
    /// "1 minute left" / "{0} seconds left" (Werewolf.cs:461-469).
    Reminder { seconds_left: i64 },
    /// `/extend` moved the timer (Werewolf.cs:476-490).
    Extended { by: i64, seconds_left: i64 },
    /// Players who joined since the last announcement (Werewolf.cs:455-459).
    AnnounceJoined,
    /// Joining is over: start the game or cancel it.
    Closed,
}

#[derive(Debug, Clone)]
pub struct JoinTimer {
    /// The legacy loop variable `i`: seconds elapsed, adjusted by joins and
    /// extends. Joining ends when it reaches `GAME_JOIN_TIME`.
    elapsed: i64,
    ticks: i64,
    last_count: usize,
    seconds_to_add: i64,
    force_start: bool,
}

impl JoinTimer {
    pub fn new(players: usize) -> Self {
        JoinTimer { elapsed: 0, ticks: 0, last_count: players, seconds_to_add: 0, force_start: false }
    }

    pub fn seconds_left(&self) -> i64 {
        GAME_JOIN_TIME - self.elapsed
    }

    /// Queues an `/extend` for the next tick, like the legacy
    /// `_secondsToAdd` field.
    pub fn extend(&mut self, seconds: i64) {
        self.seconds_to_add += seconds;
    }

    /// `/forcestart`, or the lobby filling up: close on the next tick.
    pub fn force_start(&mut self) {
        self.force_start = true;
    }

    /// One second of the legacy loop. `players` is the current count and
    /// `has_unannounced_joins` whether anyone joined since the last
    /// `AnnounceJoined`.
    pub fn tick(&mut self, players: usize, has_unannounced_joins: bool) -> Vec<TimerEvent> {
        if self.force_start || self.elapsed >= GAME_JOIN_TIME {
            return vec![TimerEvent::Closed];
        }
        let mut events = vec![];
        let i = &mut self.elapsed;

        // A join gives everyone more time: back up to 30 seconds, but never
        // to before second 120 (Werewolf.cs:449-453).
        if players != self.last_count {
            *i = (*i).min((*i - 30).max(120));
            self.last_count = players;
        }

        if self.ticks % ANNOUNCE_EVERY == 0 && has_unannounced_joins {
            events.push(TimerEvent::AnnounceJoined);
        }
        self.ticks += 1;

        if let Some(&s) = IMPORTANT_SECONDS.iter().find(|&&s| *i == GAME_JOIN_TIME - s) {
            events.push(TimerEvent::Reminder { seconds_left: s });
        }

        if self.seconds_to_add != 0 {
            *i = (*i - self.seconds_to_add).max(GAME_JOIN_TIME - MAX_JOIN_TIME);
            if GAME_JOIN_TIME > *i {
                events.push(TimerEvent::Extended { by: self.seconds_to_add, seconds_left: GAME_JOIN_TIME - *i });
            }
            self.seconds_to_add = 0;
        }

        *i += 1;
        events
    }
}

/// `mm:ss`, as the legacy `TimeSpan.ToString(@"mm\:ss")`.
pub fn mm_ss(seconds: i64) -> String {
    let s = seconds.max(0);
    format!("{:02}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_until_closed(t: &mut JoinTimer, players: usize) -> (i64, Vec<TimerEvent>) {
        let mut all = vec![];
        for second in 0..10_000 {
            let ev = t.tick(players, false);
            if ev.contains(&TimerEvent::Closed) {
                return (second, all);
            }
            all.extend(ev);
        }
        panic!("never closed");
    }

    #[test]
    fn joining_lasts_three_minutes_with_reminders() {
        let mut t = JoinTimer::new(0);
        let (secs, events) = run_until_closed(&mut t, 0);
        assert_eq!(secs, 180);
        assert_eq!(
            events,
            vec![
                TimerEvent::Reminder { seconds_left: 60 },
                TimerEvent::Reminder { seconds_left: 30 },
                TimerEvent::Reminder { seconds_left: 10 },
            ]
        );
    }

    #[test]
    fn a_late_join_buys_up_to_30_seconds_but_not_before_second_120() {
        let mut t = JoinTimer::new(0);
        for _ in 0..170 {
            t.tick(0, false);
        }
        assert_eq!(t.seconds_left(), 10);
        t.tick(1, true); // elapsed 170 -> 140, then +1
        assert_eq!(t.seconds_left(), 39);

        let mut t = JoinTimer::new(0);
        for _ in 0..130 {
            t.tick(0, false);
        }
        t.tick(1, true); // elapsed 130 -> 120, then +1
        assert_eq!(t.seconds_left(), 59);

        let mut t = JoinTimer::new(0);
        for _ in 0..50 {
            t.tick(0, false);
        }
        t.tick(1, true); // early joins change nothing
        assert_eq!(t.seconds_left(), 129);
    }

    #[test]
    fn extend_reports_the_new_time_and_is_capped_at_five_minutes() {
        let mut t = JoinTimer::new(0);
        t.extend(30);
        let ev = t.tick(0, false);
        assert_eq!(ev, vec![TimerEvent::Extended { by: 30, seconds_left: 210 }]);
        t.extend(1000);
        let ev = t.tick(0, false);
        assert_eq!(ev, vec![TimerEvent::Extended { by: 1000, seconds_left: 300 }]);
    }

    #[test]
    fn negative_extends_shorten_joining() {
        let mut t = JoinTimer::new(0);
        t.extend(-170);
        let ev = t.tick(0, false);
        assert_eq!(ev, vec![TimerEvent::Extended { by: -170, seconds_left: 10 }]);
    }

    #[test]
    fn force_start_closes_on_the_next_tick() {
        let mut t = JoinTimer::new(5);
        t.tick(5, false);
        t.force_start();
        assert_eq!(t.tick(5, false), vec![TimerEvent::Closed]);
    }

    #[test]
    fn joins_are_announced_every_30_seconds() {
        let mut t = JoinTimer::new(0);
        assert_eq!(t.tick(1, true), vec![TimerEvent::AnnounceJoined]);
        for _ in 1..30 {
            assert!(!t.tick(1, true).contains(&TimerEvent::AnnounceJoined));
        }
        assert!(t.tick(1, true).contains(&TimerEvent::AnnounceJoined));
    }

    #[test]
    fn mm_ss_formats() {
        assert_eq!(mm_ss(210), "03:30");
        assert_eq!(mm_ss(9), "00:09");
    }
}
