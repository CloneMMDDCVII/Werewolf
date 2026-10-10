//! The per-game settings the engine follows, taken from the group's
//! configuration at game start (Werewolf.cs:175-216). Telegram-free and
//! storage-free: `control::game_rules::game_start` builds one from the
//! stored `GroupSettings`, resolving Random mode first.
//!
//! Display-only settings are not here because the engine has nothing to
//! decide for them: ShowIDs, ShufflePlayerList and AllowNSFW only change
//! how a presenter draws names, buttons and GIFs.

use crate::orchestrator::AlivePlayer;
use shared::Role;

/// `Group.ShowRolesEnd`: which roles the end-of-game summary reveals
/// (Werewolf.cs:4906-4925).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndRoles {
    /// Names only.
    None,
    /// Survivors with role, team and result.
    Living,
    /// Everyone with status, role and result.
    All,
}

/// Secret lynch (`EnableSecretLynch`): votes are not announced as they are
/// cast, only counted (Werewolf.cs:1127-1136). The two flags control the
/// results shown after the vote (Werewolf.cs:2717-2741).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretLynch {
    pub show_votes: bool,
    pub show_voters: bool,
}

/// Phase lengths in seconds, as stored for the group (`Group.DayTime`,
/// `NightTime`, `LynchTime`, with `Settings.TimeDay` etc. as defaults).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timers {
    pub day_seconds: u32,
    pub night_seconds: u32,
    pub lynch_seconds: u32,
}

impl Default for Timers {
    /// Settings.cs:79-81.
    fn default() -> Self {
        Timers {
            day_seconds: 60,
            night_seconds: 90,
            lynch_seconds: 90,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameRules {
    pub show_roles_on_death: bool,
    pub end_roles: EndRoles,
    pub secret_lynch: Option<SecretLynch>,
    pub thief_full: bool,
    pub timers: Timers,
}

impl Default for GameRules {
    /// The legacy group defaults (`GroupConfig.cs`): roles shown on death
    /// and at the end, public votes, plain Thief.
    fn default() -> Self {
        GameRules {
            show_roles_on_death: true,
            end_roles: EndRoles::All,
            secret_lynch: None,
            thief_full: false,
            timers: Timers::default(),
        }
    }
}

impl GameRules {
    /// Werewolf.cs:3003-3010: night 1 lasts at least two minutes when
    /// someone has a first-night choice to make (Cupid, Doppelgänger, Wild
    /// Child, or a Thief outside full mode).
    pub fn night_seconds(&self, night: u32, alive: &[AlivePlayer]) -> u32 {
        let mut seconds = self.timers.night_seconds;
        if night == 1 {
            let first_night_choice = alive.iter().any(|p| {
                matches!(p.role, Role::Cupid | Role::Doppelganger | Role::WildChild)
                    || (p.role == Role::Thief && !self.thief_full)
            });
            if first_night_choice {
                seconds = seconds.max(120);
            }
        }
        seconds
    }

    /// Werewolf.cs:2812: bigger games get longer days,
    /// `max(((alive / 5) - 1) * 30, 60)` seconds on top of the timer.
    pub fn day_seconds(&self, alive_count: usize) -> u32 {
        let extra = ((alive_count as i64 / 5) - 1) * 30;
        self.timers.day_seconds + extra.max(60) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::PlayerId;

    fn roster(roles: &[Role]) -> Vec<AlivePlayer> {
        roles
            .iter()
            .enumerate()
            .map(|(i, &role)| AlivePlayer {
                id: PlayerId(i as u64),
                role,
            })
            .collect()
    }

    #[test]
    fn days_grow_with_the_number_of_players() {
        let rules = GameRules::default();
        assert_eq!(rules.day_seconds(8), 120);
        assert_eq!(rules.day_seconds(15), 120);
        assert_eq!(rules.day_seconds(20), 150);
        assert_eq!(rules.day_seconds(35), 240);
    }

    #[test]
    fn the_first_night_is_longer_when_someone_has_a_first_night_choice() {
        let mut rules = GameRules::default();
        rules.timers.night_seconds = 60;
        assert_eq!(rules.night_seconds(1, &roster(&[Role::Wolf, Role::Villager])), 60);
        assert_eq!(rules.night_seconds(1, &roster(&[Role::Wolf, Role::Cupid])), 120);
        assert_eq!(rules.night_seconds(2, &roster(&[Role::Wolf, Role::Cupid])), 60);
        assert_eq!(rules.night_seconds(1, &roster(&[Role::Wolf, Role::Thief])), 120);
        rules.thief_full = true;
        assert_eq!(rules.night_seconds(1, &roster(&[Role::Wolf, Role::Thief])), 60);
    }
}
