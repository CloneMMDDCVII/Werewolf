//! The end-of-game summary (Werewolf.cs:4906-4925): who was in the game,
//! who survived, and, depending on the group's `ShowRolesEnd`, their roles
//! and whether they won.

use crate::roles::PlayerId;
use crate::rules::EndRoles;
use crate::WinOutcome;
use shared::{Role, Team};

/// Every key `GameSummary::render` can look up, for the coverage check.
pub const SUMMARY_KEYS: [&str; 15] = [
    "PlayersAlive",
    "RemainingPlayersEnd",
    "Alive",
    "Dead",
    "RanAway",
    "Won",
    "Lost",
    "VillageTeamEnd",
    "WolfTeamEnd",
    "TannerTeamEnd",
    "CultTeamEnd",
    "SerialKillerTeamEnd",
    "ArsonistTeamEnd",
    "ThiefTeamEnd",
    "NeutralTeamEnd",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryEntry {
    pub id: PlayerId,
    /// Role at the end of the game, or at death.
    pub role: Role,
    pub alive: bool,
    pub fled: bool,
    pub in_love: bool,
    pub won: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSummary {
    pub end_roles: EndRoles,
    /// Survivors first, then the dead in the order they died (the legacy
    /// list is ordered by `TimeDied`, which is unset for survivors).
    pub entries: Vec<SummaryEntry>,
}

/// Whether a player with `role` (and `in_love`) is on the winning side.
/// Mirrors who `DoGameEnd` marks `Won` (Werewolf.cs:4700-4880): the
/// winning team's members, living or dead, the pair for a Lovers win, and
/// nobody when there is no winner.
pub fn is_winner(winner: &WinOutcome, role: Role, in_love: bool, lynched_tanner: bool) -> bool {
    match winner {
        WinOutcome::Team(Team::Lovers) => in_love,
        WinOutcome::Team(Team::Tanner) => role == Role::Tanner && lynched_tanner,
        WinOutcome::Team(Team::NoOne) | WinOutcome::Team(Team::Thief) => false,
        WinOutcome::Team(team) => role.team() == *team,
        WinOutcome::RngDependent(_)
        | WinOutcome::InsufficientData(_)
        | WinOutcome::Unimplemented(_)
        | WinOutcome::Killed => false,
    }
}

fn team_end_key(team: Team) -> &'static str {
    match team {
        Team::Village => "VillageTeamEnd",
        Team::Wolf => "WolfTeamEnd",
        Team::Tanner => "TannerTeamEnd",
        Team::Cult => "CultTeamEnd",
        Team::SerialKiller => "SerialKillerTeamEnd",
        Team::Arsonist => "ArsonistTeamEnd",
        Team::Thief => "ThiefTeamEnd",
        Team::Lovers | Team::NoOne => "NeutralTeamEnd",
    }
}

impl GameSummary {
    /// The summary text, built the way Werewolf.cs:4906-4925 builds it.
    pub fn render(&self, lookup: &dyn Fn(&str) -> Option<String>, name: &dyn Fn(PlayerId) -> String) -> String {
        let text = |key: &str| lookup(key).unwrap_or_else(|| format!("({key})"));
        let role = |r: Role| lookup(r.locale_key()).unwrap_or_else(|| format!("{r:?}"));
        let love = |e: &SummaryEntry| if e.in_love { "❤️" } else { "" };
        let result = |e: &SummaryEntry| text(if e.won { "Won" } else { "Lost" });
        let alive_count = self.entries.iter().filter(|e| e.alive).count();
        let header = format!("{}: {} / {}", text("PlayersAlive"), alive_count, self.entries.len());
        match self.end_roles {
            EndRoles::None => {
                let names: Vec<String> = self.entries.iter().map(|e| name(e.id)).collect();
                format!("{header}\n{}", names.join("\n"))
            }
            EndRoles::All => {
                let lines: Vec<String> = self
                    .entries
                    .iter()
                    .map(|e| {
                        let status = if e.alive {
                            text("Alive")
                        } else if e.fled {
                            text("RanAway")
                        } else {
                            text("Dead")
                        };
                        format!("{}: {status} - {}{} {}", name(e.id), role(e.role), love(e), result(e))
                    })
                    .collect();
                format!("{header}\n{}", lines.join("\n"))
            }
            EndRoles::Living => {
                let mut living: Vec<&SummaryEntry> = self.entries.iter().filter(|e| e.alive).collect();
                living.sort_by_key(|e| team_end_key(e.role.team()));
                let lines: Vec<String> = living
                    .iter()
                    .map(|e| {
                        format!(
                            "{}: {} {} {}{}",
                            name(e.id),
                            role(e.role),
                            text(team_end_key(e.role.team())),
                            love(e),
                            result(e)
                        )
                    })
                    .collect();
                format!("{}\n\n{}", text("RemainingPlayersEnd"), lines.join("\n"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, role: Role, alive: bool, won: bool) -> SummaryEntry {
        SummaryEntry {
            id: PlayerId(id),
            role,
            alive,
            fled: false,
            in_love: false,
            won,
        }
    }

    fn lookup(key: &str) -> Option<String> {
        Some(
            match key {
                "PlayersAlive" => "Players Alive",
                "RemainingPlayersEnd" => "Remaining:",
                "Alive" => "Alive",
                "Dead" => "Dead",
                "Won" => "Won",
                "Lost" => "Lost",
                "Wolf" => "a Werewolf",
                "Villager" => "a Villager",
                "WolfTeamEnd" => "Wolf Team",
                _ => return None,
            }
            .to_string(),
        )
    }

    fn summary(end_roles: EndRoles) -> GameSummary {
        GameSummary {
            end_roles,
            entries: vec![entry(1, Role::Wolf, true, true), entry(2, Role::Villager, false, false)],
        }
    }

    fn name(id: PlayerId) -> String {
        format!("P{}", id.0)
    }

    #[test]
    fn none_lists_names_only() {
        assert_eq!(
            summary(EndRoles::None).render(&lookup, &name),
            "Players Alive: 1 / 2\nP1\nP2"
        );
    }

    #[test]
    fn all_lists_everyone_with_roles_and_results() {
        assert_eq!(
            summary(EndRoles::All).render(&lookup, &name),
            "Players Alive: 1 / 2\nP1: Alive - a Werewolf Won\nP2: Dead - a Villager Lost"
        );
    }

    #[test]
    fn living_lists_survivors_with_team() {
        assert_eq!(
            summary(EndRoles::Living).render(&lookup, &name),
            "Remaining:\n\nP1: a Werewolf Wolf Team Won"
        );
    }

    #[test]
    fn winners_are_the_winning_team_or_the_lovers() {
        let wolves = WinOutcome::Team(Team::Wolf);
        assert!(is_winner(&wolves, Role::Lycan, false, false));
        assert!(!is_winner(&wolves, Role::Villager, false, false));
        let lovers = WinOutcome::Team(Team::Lovers);
        assert!(is_winner(&lovers, Role::Wolf, true, false));
        assert!(!is_winner(&lovers, Role::Wolf, false, false));
        assert!(!is_winner(&WinOutcome::Killed, Role::Villager, false, false));
    }
}
