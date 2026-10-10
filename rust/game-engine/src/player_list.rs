//! `Werewolf.cs::SendPlayerList` (1352-1408), the content-building half
//! of `/players` (`GameCommands.cs::Players` -> `GameInfo.ShowPlayers()`
//! -> `Werewolf.cs::OutputPlayers()`, which - once a player-list message
//! already exists - just re-points to it, throttled to once per 10
//! seconds, rather than rebuilding it every time). This module ports the
//! *content*: given the current roster, who's died and how, and whether
//! the group reveals roles on death, produce the same per-player status
//! the real list shows.
//!
//! Deliberately not ported: the Telegram message plumbing (an existing
//! player-list message that gets edited in place rather than resent,
//! `OutputPlayers`'s 10-second throttle, the "LatestList"/
//! "UnableToGetList" reply-pointer dance - all delivery concerns for
//! whatever real presenter renders this) and the `ShufflePlayerList`
//! anonymized-order variant (Werewolf.cs:1374-1382, plus its Valentine's
//! Day love-emoji easter egg) - a cosmetic alternate rendering, not part
//! of "what does the list actually say."

use crate::roles::PlayerId;
use shared::{KillMethod, Role};

/// One roster entry going into [`render_player_list`] - `name`/`role` as
/// of the moment they're being listed. `role` should be each player's
/// *current* role (Werewolf.cs uses `p.PlayerRole` at render time, not a
/// starting assignment - a Traitor who's turned Wolf by the time this
/// renders shows as `Wolf`, matching the source rather than a snapshot
/// from game start).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterEntry {
    pub id: PlayerId,
    pub name: String,
    pub role: Role,
    pub alive: bool,
    /// `None` for a still-alive player. For a dead one, the method they
    /// died by - used only to recover Werewolf.cs's `p.Fled` flag
    /// (`KillMethod::Flee` is the sole source of `Fled = true`,
    /// Werewolf.cs:849-851, so there's no need to track it as a separate
    /// bool here).
    pub death_method: Option<KillMethod>,
    /// Werewolf.cs's `p.InLove` - only ever shown alongside a role
    /// reveal (see `DeathReveal`), never on its own.
    pub was_in_love: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerListEntry {
    pub id: PlayerId,
    pub name: String,
    pub status: PlayerStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerStatus {
    Alive,
    /// Werewolf.cs:1394: `p.Fled ? "RanAway" : "Dead"`, plus a role
    /// reveal that only happens when the group has `ShowRolesDeath` set
    /// (checked directly: the role-and-love-emoji clause is nested
    /// *inside* that same `ShowRolesDeath` condition, not shown
    /// unconditionally).
    Dead {
        fled: bool,
        revealed: Option<DeathReveal>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeathReveal {
    pub role: Role,
    pub was_in_love: bool,
}

/// Builds the same per-player status Werewolf.cs's non-shuffled
/// `SendPlayerList` shows (the `else` branch, 1388-1394) - preserves
/// `roster`'s given order rather than the source's `OrderBy(x =>
/// x.TimeDied)` (the engine has no `TimeDied` to sort by;
/// ordering is left to the caller, who has whatever ordering context it
/// tracked getting here).
pub fn render_player_list(roster: &[RosterEntry], reveal_roles_on_death: bool) -> Vec<PlayerListEntry> {
    roster
        .iter()
        .map(|p| PlayerListEntry {
            id: p.id,
            name: p.name.clone(),
            status: if p.alive {
                PlayerStatus::Alive
            } else {
                PlayerStatus::Dead {
                    fled: p.death_method == Some(KillMethod::Flee),
                    revealed: reveal_roles_on_death.then_some(DeathReveal {
                        role: p.role,
                        was_in_love: p.was_in_love,
                    }),
                }
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alive(id: u64, name: &str, role: Role) -> RosterEntry {
        RosterEntry {
            id: PlayerId(id),
            name: name.to_string(),
            role,
            alive: true,
            death_method: None,
            was_in_love: false,
        }
    }

    fn dead(id: u64, name: &str, role: Role, method: KillMethod, in_love: bool) -> RosterEntry {
        RosterEntry {
            id: PlayerId(id),
            name: name.to_string(),
            role,
            alive: false,
            death_method: Some(method),
            was_in_love: in_love,
        }
    }

    #[test]
    fn alive_players_show_as_alive_regardless_of_reveal_setting() {
        let roster = vec![alive(1, "Alice", Role::Seer)];
        for reveal in [true, false] {
            let list = render_player_list(&roster, reveal);
            assert_eq!(list[0].status, PlayerStatus::Alive);
        }
    }

    #[test]
    fn a_dead_player_with_no_reveal_setting_shows_no_role() {
        let roster = vec![dead(1, "Alice", Role::Seer, KillMethod::Eat, false)];
        let list = render_player_list(&roster, false);
        assert_eq!(
            list[0].status,
            PlayerStatus::Dead {
                fled: false,
                revealed: None
            }
        );
    }

    #[test]
    fn a_dead_player_with_reveal_shows_role_and_love_status() {
        let roster = vec![dead(1, "Alice", Role::Seer, KillMethod::Eat, true)];
        let list = render_player_list(&roster, true);
        assert_eq!(
            list[0].status,
            PlayerStatus::Dead {
                fled: false,
                revealed: Some(DeathReveal {
                    role: Role::Seer,
                    was_in_love: true
                })
            }
        );
    }

    #[test]
    fn fleeing_is_reported_distinctly_from_dying() {
        let roster = vec![
            dead(1, "Alice", Role::Villager, KillMethod::Flee, false),
            dead(2, "Bob", Role::Villager, KillMethod::Eat, false),
        ];
        let list = render_player_list(&roster, false);
        assert_eq!(
            list[0].status,
            PlayerStatus::Dead {
                fled: true,
                revealed: None
            }
        );
        assert_eq!(
            list[1].status,
            PlayerStatus::Dead {
                fled: false,
                revealed: None
            }
        );
    }

    #[test]
    fn preserves_roster_order() {
        let roster = vec![
            alive(3, "Carl", Role::Wolf),
            alive(1, "Alice", Role::Seer),
            alive(2, "Bob", Role::Hunter),
        ];
        let list = render_player_list(&roster, false);
        let ids: Vec<u64> = list.iter().map(|p| p.id.0).collect();
        assert_eq!(ids, vec![3, 1, 2]);
    }
}
