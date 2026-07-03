//! A single group's join-phase state machine (Werewolf.cs's `AddPlayer`/
//! `RemovePlayer`/`ForceStart`/`Kill`, 685-843 and 2517-2530) — the layer
//! above `game_engine::run_game`: this crate decides *whether* and *with
//! whom* a game starts; the engine only ever sees a roster that already
//! cleared every check here.
//!
//! **Scoped to the joining phase only.** Three real mechanics are
//! deliberately left out, honestly, not silently:
//! - `ExtendTime` (Werewolf.cs:2521-2534): adding seconds to the join
//!   timer is tightly coupled to the real timer's exact polling loop
//!   (`Thread.Sleep(1000)` ticking a mutable `i`, Werewolf.cs:410-501) —
//!   there's no clean state to extend without modeling that loop itself,
//!   which belongs to whatever real scheduler eventually drives this
//!   (Telegram's bot runtime, a `tokio::time::interval`, ...), not this
//!   crate's pure state machine.
//! - Flee-while-running (Werewolf.cs:848-860, the `IsRunning` branch of
//!   `RemovePlayer`): killing a player mid-game needs `game_engine
//!   ::run_game` to accept an external "this player left" event during
//!   an in-progress night/day, which it doesn't have a seam for yet.
//! - Killing an in-progress game (Werewolf.cs:5393-5406, `Kill()` when
//!   `IsRunning`): same gap — there's no cancellation seam into
//!   `run_game` once it's started. `Lobby::kill` only covers the joining
//!   phase, where "kill" and "there's no game to interrupt yet" are the
//!   same thing.

use std::collections::HashSet;

/// A player's identity as seen during joining — just enough to run the
/// real `AddPlayer` checks, not the full `AlivePlayer` (which needs a
/// role, decided only once a game actually starts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub telegram_id: i64,
    pub name: String,
}

/// Werewolf.cs `Settings.cs:76`: the release-mode default (the `#if
/// DEBUG` branch drops it to 1, purely for local testing, not a real
/// game rule).
pub const DEFAULT_MIN_PLAYERS: usize = 5;
/// Werewolf.cs `Settings.cs:78`.
pub const DEFAULT_MAX_PLAYERS: usize = 35;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LobbyState {
    Joining,
    Started,
    Aborted(AbortReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbortReason {
    /// Werewolf.cs:511-515: the join timer (or a force-start) closed
    /// joining, but fewer than `min_players` had joined.
    NotEnoughPlayers,
    /// Werewolf.cs:5393-5406: an admin killed the lobby outright.
    Killed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinError {
    /// Werewolf.cs:690-694: joining is only possible in `Joining` (not
    /// `IsInitializing`, not running, not aborted).
    NotJoining,
    /// Werewolf.cs:694-698.
    AlreadyJoined,
    /// Werewolf.cs:719-724: empty, whitespace-only, starts with `/`, or
    /// is (case-insensitively) "skip" - all real player-facing menu
    /// commands/reserved words a name can't collide with.
    InvalidName,
    /// Werewolf.cs:725-729: two players can't share a display name (the
    /// game refers to players by name in most messages).
    NameTaken,
    /// Werewolf.cs:729-733.
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaveError {
    /// Werewolf.cs:832: leaving (during joining) only makes sense while
    /// still `Joining` - this proof-of-concept doesn't model the
    /// running-game flee path (see module doc).
    NotJoining,
    NotInGame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOutcome {
    Started,
    Aborted(AbortReason),
}

/// A single group's pre-game roster and join-phase rules. One `Lobby`
/// per game session, the same lifetime as one `Werewolf` instance in the
/// legacy code, but trimmed to just the joining phase (see module doc).
#[derive(Debug, Clone)]
pub struct Lobby {
    state: LobbyState,
    players: Vec<Candidate>,
    min_players: usize,
    max_players: usize,
    /// Werewolf.cs's `KillTimer` flag (2517-2519, 743-744): set either by
    /// an explicit force-start or by hitting `max_players`, and checked
    /// by whatever real timer loop drives this `Lobby` to mean "stop
    /// waiting, resolve now" - `try_start` is that resolution.
    close_requested: bool,
}

impl Lobby {
    pub fn new() -> Self {
        Self::with_limits(DEFAULT_MIN_PLAYERS, DEFAULT_MAX_PLAYERS)
    }

    pub fn with_limits(min_players: usize, max_players: usize) -> Self {
        Lobby {
            state: LobbyState::Joining,
            players: vec![],
            min_players,
            max_players,
            close_requested: false,
        }
    }

    pub fn state(&self) -> LobbyState {
        self.state
    }

    pub fn players(&self) -> &[Candidate] {
        &self.players
    }

    /// Werewolf.cs:685-761 (`AddPlayer`), trimmed to the checks that
    /// don't need Telegram/DB I/O (a DB player record, PM reachability,
    /// achievement bitmasks, ...) - every one of those either doesn't
    /// gate joining at all in the source or is infrastructure this crate
    /// has no business owning.
    pub fn join(&mut self, candidate: Candidate) -> Result<(), JoinError> {
        if self.state != LobbyState::Joining {
            return Err(JoinError::NotJoining);
        }
        if self.players.iter().any(|p| p.telegram_id == candidate.telegram_id) {
            return Err(JoinError::AlreadyJoined);
        }
        let trimmed = candidate.name.trim();
        if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.eq_ignore_ascii_case("skip") {
            return Err(JoinError::InvalidName);
        }
        if self.players.iter().any(|p| p.name == trimmed) {
            return Err(JoinError::NameTaken);
        }
        if self.players.len() >= self.max_players {
            return Err(JoinError::Full);
        }

        self.players.push(Candidate { telegram_id: candidate.telegram_id, name: trimmed.to_string() });

        // Werewolf.cs:743-744: hitting the cap ends joining immediately,
        // the same signal a force-start sends.
        if self.players.len() == self.max_players {
            self.close_requested = true;
        }

        Ok(())
    }

    /// Werewolf.cs:829-869 (`RemovePlayer`), the `IsJoining` branch only
    /// (861-865) - see the module doc for why the running-game branch
    /// isn't modeled here.
    pub fn leave(&mut self, telegram_id: i64) -> Result<(), LeaveError> {
        if self.state != LobbyState::Joining {
            return Err(LeaveError::NotJoining);
        }
        let before = self.players.len();
        self.players.retain(|p| p.telegram_id != telegram_id);
        if self.players.len() == before {
            return Err(LeaveError::NotInGame);
        }
        Ok(())
    }

    /// Werewolf.cs:2517-2519 (`ForceStart`): just raises the same signal
    /// hitting `max_players` already does - resolving it into an actual
    /// state transition is `try_start`'s job, run whenever the caller's
    /// real timer loop notices `should_close_for_joining()`.
    pub fn force_start(&mut self) {
        self.close_requested = true;
    }

    pub fn should_close_for_joining(&self) -> bool {
        self.close_requested
    }

    /// Werewolf.cs:501-517: once joining closes (timer expiry or
    /// `close_requested`), fewer than `min_players` aborts the game
    /// entirely rather than starting short-handed. Idempotent once
    /// resolved - calling this again after `Started`/`Aborted` just
    /// re-reports the same outcome rather than re-deciding anything.
    pub fn try_start(&mut self) -> StartOutcome {
        match self.state {
            LobbyState::Started => return StartOutcome::Started,
            LobbyState::Aborted(reason) => return StartOutcome::Aborted(reason),
            LobbyState::Joining => {}
        }

        if self.players.len() < self.min_players {
            self.state = LobbyState::Aborted(AbortReason::NotEnoughPlayers);
            StartOutcome::Aborted(AbortReason::NotEnoughPlayers)
        } else {
            self.state = LobbyState::Started;
            StartOutcome::Started
        }
    }

    /// Werewolf.cs:5393-5406 (`Kill`), the joining-phase case only - see
    /// the module doc for why an already-`Started` lobby can't be killed
    /// from here.
    pub fn kill(&mut self) -> Result<(), LobbyState> {
        if self.state != LobbyState::Joining {
            return Err(self.state);
        }
        self.state = LobbyState::Aborted(AbortReason::Killed);
        Ok(())
    }

    /// The roster to hand to `game_engine::run_game`, once `try_start`
    /// has returned `Started` - every `telegram_id` is guaranteed unique
    /// (enforced at join time), which is all a caller building
    /// `PlayerId`s needs to know.
    pub fn telegram_ids(&self) -> HashSet<i64> {
        self.players.iter().map(|p| p.telegram_id).collect()
    }
}

impl Default for Lobby {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: i64, name: &str) -> Candidate {
        Candidate { telegram_id: id, name: name.to_string() }
    }

    #[test]
    fn joining_adds_a_player() {
        let mut lobby = Lobby::new();
        assert_eq!(lobby.join(candidate(1, "Alice")), Ok(()));
        assert_eq!(lobby.players().len(), 1);
    }

    #[test]
    fn cannot_join_twice() {
        let mut lobby = Lobby::new();
        lobby.join(candidate(1, "Alice")).unwrap();
        assert_eq!(lobby.join(candidate(1, "Alice Again")), Err(JoinError::AlreadyJoined));
    }

    #[test]
    fn rejects_slash_names_empty_names_and_skip() {
        let mut lobby = Lobby::new();
        assert_eq!(lobby.join(candidate(1, "/start")), Err(JoinError::InvalidName));
        assert_eq!(lobby.join(candidate(2, "   ")), Err(JoinError::InvalidName));
        assert_eq!(lobby.join(candidate(3, "Skip")), Err(JoinError::InvalidName));
        assert_eq!(lobby.join(candidate(4, "SKIP")), Err(JoinError::InvalidName));
        assert_eq!(lobby.players().len(), 0);
    }

    #[test]
    fn rejects_duplicate_names_from_different_players() {
        let mut lobby = Lobby::new();
        lobby.join(candidate(1, "Alice")).unwrap();
        assert_eq!(lobby.join(candidate(2, "Alice")), Err(JoinError::NameTaken));
    }

    #[test]
    fn rejects_joins_past_max_players() {
        let mut lobby = Lobby::with_limits(1, 2);
        lobby.join(candidate(1, "Alice")).unwrap();
        lobby.join(candidate(2, "Bob")).unwrap();
        assert_eq!(lobby.join(candidate(3, "Carl")), Err(JoinError::Full));
    }

    #[test]
    fn hitting_max_players_requests_an_early_close() {
        let mut lobby = Lobby::with_limits(1, 2);
        assert!(!lobby.should_close_for_joining());
        lobby.join(candidate(1, "Alice")).unwrap();
        assert!(!lobby.should_close_for_joining());
        lobby.join(candidate(2, "Bob")).unwrap();
        assert!(lobby.should_close_for_joining());
    }

    #[test]
    fn leaving_during_joining_removes_the_player() {
        let mut lobby = Lobby::new();
        lobby.join(candidate(1, "Alice")).unwrap();
        assert_eq!(lobby.leave(1), Ok(()));
        assert_eq!(lobby.players().len(), 0);
    }

    #[test]
    fn leaving_a_player_who_never_joined_is_an_error() {
        let mut lobby = Lobby::new();
        assert_eq!(lobby.leave(1), Err(LeaveError::NotInGame));
    }

    #[test]
    fn a_name_freed_by_leaving_can_be_reused() {
        let mut lobby = Lobby::new();
        lobby.join(candidate(1, "Alice")).unwrap();
        lobby.leave(1).unwrap();
        assert_eq!(lobby.join(candidate(2, "Alice")), Ok(()));
    }

    #[test]
    fn too_few_players_aborts_instead_of_starting() {
        let mut lobby = Lobby::with_limits(5, 35);
        lobby.join(candidate(1, "Alice")).unwrap();
        lobby.force_start();

        assert_eq!(lobby.try_start(), StartOutcome::Aborted(AbortReason::NotEnoughPlayers));
        assert_eq!(lobby.state(), LobbyState::Aborted(AbortReason::NotEnoughPlayers));
    }

    #[test]
    fn enough_players_starts_the_game() {
        let mut lobby = Lobby::with_limits(2, 35);
        lobby.join(candidate(1, "Alice")).unwrap();
        lobby.join(candidate(2, "Bob")).unwrap();

        assert_eq!(lobby.try_start(), StartOutcome::Started);
        assert_eq!(lobby.state(), LobbyState::Started);
    }

    #[test]
    fn try_start_is_idempotent_once_resolved() {
        let mut lobby = Lobby::with_limits(1, 35);
        lobby.join(candidate(1, "Alice")).unwrap();
        assert_eq!(lobby.try_start(), StartOutcome::Started);
        // Calling it again shouldn't re-run the min-player check against
        // whatever the roster looks like now.
        assert_eq!(lobby.try_start(), StartOutcome::Started);
    }

    #[test]
    fn cannot_join_once_started() {
        let mut lobby = Lobby::with_limits(1, 35);
        lobby.join(candidate(1, "Alice")).unwrap();
        lobby.try_start();
        assert_eq!(lobby.join(candidate(2, "Bob")), Err(JoinError::NotJoining));
    }

    #[test]
    fn cannot_leave_once_started() {
        let mut lobby = Lobby::with_limits(1, 35);
        lobby.join(candidate(1, "Alice")).unwrap();
        lobby.try_start();
        assert_eq!(lobby.leave(1), Err(LeaveError::NotJoining));
    }

    #[test]
    fn killing_during_joining_aborts_the_lobby() {
        let mut lobby = Lobby::new();
        lobby.join(candidate(1, "Alice")).unwrap();
        assert_eq!(lobby.kill(), Ok(()));
        assert_eq!(lobby.state(), LobbyState::Aborted(AbortReason::Killed));
    }

    #[test]
    fn cannot_join_a_killed_lobby() {
        let mut lobby = Lobby::new();
        lobby.kill().unwrap();
        assert_eq!(lobby.join(candidate(1, "Alice")), Err(JoinError::NotJoining));
    }

    #[test]
    fn killing_an_already_started_lobby_is_rejected() {
        let mut lobby = Lobby::with_limits(1, 35);
        lobby.join(candidate(1, "Alice")).unwrap();
        lobby.try_start();
        assert_eq!(lobby.kill(), Err(LobbyState::Started));
    }

    #[test]
    fn telegram_ids_reflects_the_current_roster() {
        let mut lobby = Lobby::new();
        lobby.join(candidate(1, "Alice")).unwrap();
        lobby.join(candidate(2, "Bob")).unwrap();
        let ids = lobby.telegram_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
    }
}
