//! A single group's join-phase state machine (Werewolf.cs's `AddPlayer`/
//! `RemovePlayer`/`ForceStart`/`Kill`, 685-843 and 2517-2530) — the layer
//! above `game_engine::run_game`: this crate decides *whether* and *with
//! whom* a game starts; the engine only ever sees a roster that already
//! cleared every check here.
//!
//! **Scoped to the joining phase only.** `Lobby::extend_time` covers
//! `ExtendTime`'s own gating rules (Werewolf.cs:2521-2534) but not the
//! timer loop itself, and `Lobby::kill` only covers the joining-phase
//! case of `Kill()` (Werewolf.cs:5393-5406) — both left honestly
//! incomplete rather than silently:
//! - The join timer's actual countdown (`Thread.Sleep(1000)` ticking a
//!   mutable `i`, Werewolf.cs:410-501) belongs to whatever real
//!   scheduler eventually drives this (Telegram's bot runtime, a
//!   `tokio::time::interval`, ...), not this crate's pure state machine
//!   — `extend_time` returns the signed delta a caller's timer should
//!   apply, but doesn't apply it to anything itself.
//! - Killing an *in-progress* game (`Kill()` when `IsRunning`) needs the
//!   same kind of external-event seam `game_engine::run_game` just
//!   grew for flee (`Presenter::drain_fled_players`) — extending that
//!   pattern to "end the game outright, no winner" is real, tracked
//!   work (see `COMMANDS.md`'s `killgame` entry), just not done yet.
//!
//! Flee-while-running (Werewolf.cs:848-860) *is* covered now, just not
//! by this module — `game_engine::game::apply_fled_players` (fed by
//! `Presenter::drain_fled_players`) is `run_game`'s side of it. A
//! `Lobby` only exists before a game starts; once `try_start` returns
//! `Started`, this module's job is done and the engine takes over.
//!
//! [`Lobby::assign_roles`] is the last step of that handoff —
//! `game_engine::balancing::balance` picks which role each joined player
//! gets (honoring `game_mode`), and this just zips the result against
//! `players()` to build the `AlivePlayer` roster `game_engine::run_game`
//! actually takes. `startgame`/`startchaos` finally resolve end-to-end
//! through this: `Lobby` decides who's playing, `balancing` decides who's
//! playing *what*, `run_game` plays it out.

use game_engine::balancing::{balance, BalanceError};
use game_engine::orchestrator::AlivePlayer;
use game_engine::rng::RngSource;
use game_engine::roles::PlayerId;
use shared::{GameMode, Role};
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
/// `Werewolf Control`'s `Helpers/Settings.cs:179` (a *different* Settings
/// class from the game engine's own, in the bot-command project — the cap
/// on how many seconds a single `/extend` can move the timer, regardless
/// of what the caller asked for).
pub const DEFAULT_MAX_EXTEND_SECONDS: i32 = 60;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtendError {
    /// Werewolf.cs:2523: `ExtendTime` itself only does anything while
    /// still `Joining`.
    NotJoining,
    /// `GameCommands.cs::Extend`: a negative (time-*removing*) request
    /// from a non-admin - "otherwise we're allowing people to
    /// /forcestart" is the source's own comment for why this is
    /// admin-only.
    NegativeRequiresAdmin,
    /// Both `GameCommands.cs::Extend` (the surrounding command's own
    /// "are you even in this game" check) and `Werewolf.cs::ExtendTime`
    /// (`Players.FirstOrDefault(x => x.TeleUser.Id == id)`) require the
    /// requester to be an actual joined player - even a group admin who
    /// hasn't joined can't extend, matching the real `ExtendTime`'s own
    /// lookup silently no-opping for exactly that case, made explicit
    /// here as an error instead.
    NotInGame,
    /// Werewolf.cs:2528-2531: a non-admin gets exactly one extend per
    /// game (`HaveExtended`); admins can extend as many times as they
    /// like.
    AlreadyExtended,
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
    /// Werewolf.cs's `HaveExtended` (a `HashSet<long>` there too) - every
    /// non-admin who has already used their one `/extend`.
    have_extended: HashSet<i64>,
    /// `/startgame` vs `/startchaos` (`GameCommands.cs::StartGame`/
    /// `StartChaos`, both calling the same `Helpers.cs::StartGame
    /// (GameMode, Update)` with a different `GameMode`). Tracked here
    /// since it's decided at lobby-creation time, before a single player
    /// has joined - but **purely metadata for now**: the only place
    /// `GameMode` actually changes anything in the source is
    /// `Werewolf.cs:1458`, `GameBalancing.Balance(..., GameMode ==
    /// GameMode.Chaos, ...)`, which decides *which roles* get handed
    /// out for N players. That role-balancing algorithm isn't ported at
    /// all yet - `run_game` only ever runs a roster with roles already
    /// assigned - so `game_mode()` has nothing downstream to affect
    /// until it exists. An honest placeholder, not a finished feature.
    game_mode: GameMode,
}

impl Lobby {
    pub fn new() -> Self {
        Self::with_limits(DEFAULT_MIN_PLAYERS, DEFAULT_MAX_PLAYERS)
    }

    pub fn with_limits(min_players: usize, max_players: usize) -> Self {
        Self::with_limits_and_mode(min_players, max_players, GameMode::Normal)
    }

    pub fn with_game_mode(game_mode: GameMode) -> Self {
        Self::with_limits_and_mode(DEFAULT_MIN_PLAYERS, DEFAULT_MAX_PLAYERS, game_mode)
    }

    pub fn with_limits_and_mode(min_players: usize, max_players: usize, game_mode: GameMode) -> Self {
        Lobby {
            state: LobbyState::Joining,
            players: vec![],
            min_players,
            max_players,
            close_requested: false,
            have_extended: HashSet::new(),
            game_mode,
        }
    }

    pub fn state(&self) -> LobbyState {
        self.state
    }

    pub fn game_mode(&self) -> GameMode {
        self.game_mode
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

    /// Werewolf.cs:2521-2534 (`ExtendTime`) plus `GameCommands.cs::Extend`'s
    /// own gating (the negative-requires-admin rule and the
    /// `max_extend_seconds` clamp - `Werewolf Control`'s command layer,
    /// not the game engine, owns both). Returns the clamped delta a
    /// caller's real timer loop should apply to its own countdown - this
    /// `Lobby` has no countdown of its own to adjust (see module doc).
    ///
    /// `max_extend_seconds` is a parameter rather than a hardcoded
    /// constant since it's genuinely a per-group setting in the source
    /// (`grp.MaxExtend ?? Settings.MaxExtend`, `GameCommands.cs:178`) -
    /// unlike most settings elsewhere in this port, a caller here already
    /// has to look up *something* group-specific to build a full `/extend`
    /// handler (whether the group even allows extending at all,
    /// `GroupConfig.AllowExtend`, isn't modeled here either - same
    /// "no group config store yet" gap `evaluate_winner_with_kills` and
    /// friends already have), so plumbing the one number through costs
    /// nothing extra.
    pub fn extend_time(
        &mut self,
        requester_id: i64,
        is_admin: bool,
        requested_seconds: i32,
        max_extend_seconds: i32,
    ) -> Result<i32, ExtendError> {
        if self.state != LobbyState::Joining {
            return Err(ExtendError::NotJoining);
        }
        // Checked before the negative/admin rule below, matching the
        // source's own order: `GameCommands.cs::Extend`'s "are you even
        // in this game" check runs first for non-admins, then
        // `ExtendTime`'s own player lookup runs regardless of admin
        // status - an admin who isn't a joined player passes the first
        // but fails the second, silently in the source (`ExtendTime`
        // just no-ops), surfaced here as the same explicit error instead
        // (see `ExtendError::NotInGame`'s own doc).
        if !self.players.iter().any(|p| p.telegram_id == requester_id) {
            return Err(ExtendError::NotInGame);
        }
        if requested_seconds < 0 && !is_admin {
            return Err(ExtendError::NegativeRequiresAdmin);
        }
        if !is_admin && self.have_extended.contains(&requester_id) {
            return Err(ExtendError::AlreadyExtended);
        }

        let clamped = if requested_seconds.abs() > max_extend_seconds {
            max_extend_seconds * requested_seconds.signum()
        } else {
            requested_seconds
        };

        self.have_extended.insert(requester_id);
        Ok(clamped)
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

    /// Werewolf.cs:1458: `GameBalancing.Balance(roleflags, Players.Count,
    /// GameMode == Shared.GameMode.Chaos, BurningOverkill, out
    /// PossibleRoles)`, immediately followed by assigning the shuffled
    /// result to the (already-shuffled-into-join-order) player list —
    /// there's no separate "which player gets which role" shuffle in the
    /// source beyond `balance`'s own internal one, so zipping
    /// `players()` against `balance`'s output in order is faithful, not
    /// a shortcut.
    ///
    /// Only callable once `try_start` has actually returned `Started` —
    /// asking "what roles" before "is this even a game" has been decided
    /// isn't a real question the source ever asks either (`Balance` is
    /// called from deep inside the same `IsInitializing` block that
    /// already passed the `MinPlayers` check).
    pub fn assign_roles(
        &self,
        disabled_roles: &[Role],
        burning_overkill: bool,
        rng: &mut dyn RngSource,
    ) -> Result<Vec<AlivePlayer>, AssignRolesError> {
        if self.state != LobbyState::Started {
            return Err(AssignRolesError::NotStarted);
        }

        let roles = balance(self.players.len(), self.game_mode == GameMode::Chaos, disabled_roles, burning_overkill, rng)?;

        Ok(self
            .players
            .iter()
            .zip(roles)
            .map(|(player, role)| AlivePlayer { id: PlayerId(player.telegram_id as u64), role })
            .collect())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignRolesError {
    /// Asked before `try_start` resolved to `Started` - there's no
    /// finished roster to assign roles to yet.
    NotStarted,
    Balance(BalanceError),
}

impl From<BalanceError> for AssignRolesError {
    fn from(e: BalanceError) -> Self {
        AssignRolesError::Balance(e)
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

    #[test]
    fn a_joined_player_can_extend_the_timer() {
        let mut lobby = Lobby::new();
        lobby.join(candidate(1, "Alice")).unwrap();
        assert_eq!(lobby.extend_time(1, false, 30, DEFAULT_MAX_EXTEND_SECONDS), Ok(30));
    }

    #[test]
    fn extend_is_clamped_to_max_extend_seconds() {
        let mut lobby = Lobby::new();
        lobby.join(candidate(1, "Alice")).unwrap();
        assert_eq!(lobby.extend_time(1, false, 999, DEFAULT_MAX_EXTEND_SECONDS), Ok(60));
    }

    #[test]
    fn a_negative_extend_is_also_clamped() {
        let mut lobby = Lobby::new();
        lobby.join(candidate(1, "Alice")).unwrap();
        assert_eq!(lobby.extend_time(1, true, -999, DEFAULT_MAX_EXTEND_SECONDS), Ok(-60));
    }

    #[test]
    fn a_non_admin_cannot_extend_negatively() {
        let mut lobby = Lobby::new();
        lobby.join(candidate(1, "Alice")).unwrap();
        assert_eq!(
            lobby.extend_time(1, false, -10, DEFAULT_MAX_EXTEND_SECONDS),
            Err(ExtendError::NegativeRequiresAdmin)
        );
    }

    #[test]
    fn an_admin_can_extend_negatively() {
        let mut lobby = Lobby::new();
        lobby.join(candidate(1, "Alice")).unwrap();
        assert_eq!(lobby.extend_time(1, true, -10, DEFAULT_MAX_EXTEND_SECONDS), Ok(-10));
    }

    #[test]
    fn a_non_player_cannot_extend_even_as_admin() {
        let mut lobby = Lobby::new();
        lobby.join(candidate(1, "Alice")).unwrap();
        assert_eq!(
            lobby.extend_time(99, true, 30, DEFAULT_MAX_EXTEND_SECONDS),
            Err(ExtendError::NotInGame)
        );
    }

    #[test]
    fn a_non_admin_can_only_extend_once() {
        let mut lobby = Lobby::new();
        lobby.join(candidate(1, "Alice")).unwrap();
        assert_eq!(lobby.extend_time(1, false, 30, DEFAULT_MAX_EXTEND_SECONDS), Ok(30));
        assert_eq!(
            lobby.extend_time(1, false, 30, DEFAULT_MAX_EXTEND_SECONDS),
            Err(ExtendError::AlreadyExtended)
        );
    }

    #[test]
    fn an_admin_can_extend_repeatedly() {
        let mut lobby = Lobby::new();
        lobby.join(candidate(1, "Alice")).unwrap();
        assert_eq!(lobby.extend_time(1, true, 30, DEFAULT_MAX_EXTEND_SECONDS), Ok(30));
        assert_eq!(lobby.extend_time(1, true, 30, DEFAULT_MAX_EXTEND_SECONDS), Ok(30));
    }

    #[test]
    fn cannot_extend_once_started() {
        let mut lobby = Lobby::with_limits(1, 35);
        lobby.join(candidate(1, "Alice")).unwrap();
        lobby.try_start();
        assert_eq!(
            lobby.extend_time(1, false, 30, DEFAULT_MAX_EXTEND_SECONDS),
            Err(ExtendError::NotJoining)
        );
    }

    #[test]
    fn default_lobbies_are_normal_mode() {
        assert_eq!(Lobby::new().game_mode(), GameMode::Normal);
    }

    #[test]
    fn a_chaos_lobby_reports_chaos_mode() {
        assert_eq!(Lobby::with_game_mode(GameMode::Chaos).game_mode(), GameMode::Chaos);
    }

    #[test]
    fn game_mode_survives_join_leave_and_start() {
        let mut lobby = Lobby::with_limits_and_mode(1, 35, GameMode::Chaos);
        lobby.join(candidate(1, "Alice")).unwrap();
        lobby.try_start();
        assert_eq!(lobby.game_mode(), GameMode::Chaos);
    }

    fn started_lobby(player_count: i64) -> Lobby {
        let mut lobby = Lobby::with_limits(1, 35);
        for id in 1..=player_count {
            lobby.join(candidate(id, &format!("Player{id}"))).unwrap();
        }
        lobby.try_start();
        lobby
    }

    #[test]
    fn cannot_assign_roles_before_starting() {
        let mut lobby = Lobby::new();
        lobby.join(candidate(1, "Alice")).unwrap();
        let mut rng = game_engine::rng::SeededRngSource::new(0);
        assert_eq!(lobby.assign_roles(&[], true, &mut rng).unwrap_err(), AssignRolesError::NotStarted);
    }

    #[test]
    fn assigns_a_role_to_every_joined_player() {
        let lobby = started_lobby(6);
        let mut rng = game_engine::rng::SeededRngSource::new(0);
        let roster = lobby.assign_roles(&[], true, &mut rng).expect("6p game should balance");

        assert_eq!(roster.len(), 6);
        let mut ids: Vec<_> = roster.iter().map(|p| p.id.0).collect();
        ids.sort();
        assert_eq!(ids, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn chaos_lobbies_pass_chaos_mode_through_to_balance() {
        let lobby = {
            let mut lobby = Lobby::with_limits_and_mode(1, 35, GameMode::Chaos);
            for id in 1..=8 {
                lobby.join(candidate(id, &format!("Player{id}"))).unwrap();
            }
            lobby.try_start();
            lobby
        };
        let mut rng = game_engine::rng::SeededRngSource::new(0);
        let roster = lobby.assign_roles(&[], true, &mut rng).expect("8p chaos game should balance");
        assert_eq!(roster.len(), 8);
    }
}
