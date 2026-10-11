//! The running game's end check, ported from `CheckForGameEnd`
//! (Werewolf.cs:4477-4636) and the endings `DoGameEnd` plays out
//! (4638-4900): the last-two standoffs (Hunter against a wolf or the
//! Serial Killer, the Cultist Hunter against a Cultist, the cult
//! auto-convert), the Serial Killer's and Arsonist's overpower kills, the
//! neutral-role endings, and the Gunner who can still save the village.
//!
//! `lib::evaluate_winner` stays as the RNG-free analysis the fixture
//! replays use; this module is what a live game follows. The Tanner-lynch
//! win is checked before this, from the kill history.

use crate::messages::Announcement;
use crate::orchestrator::AlivePlayer;
use crate::rng::RngSource;
use crate::roles::PlayerId;
use shared::{KillMethod, Role, Team};

/// `Settings.HunterKillWolfChanceBase`.
pub const HUNTER_KILLS_WOLF_PERCENT: u32 = 30;

/// What one player knows about the others that the end check needs.
pub struct EndContext<'a> {
    pub alive: &'a [AlivePlayer],
    pub in_love: &'a dyn Fn(PlayerId) -> bool,
    pub bullets: &'a dyn Fn(PlayerId) -> u8,
    /// A Doppelgänger's role model.
    pub role_model: &'a dyn Fn(PlayerId) -> Option<PlayerId>,
    /// Players bitten by the Alpha Wolf who turn next night; the village
    /// can't win while one is alive (Werewolf.cs:4631).
    pub bitten: &'a [PlayerId],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameEnd {
    pub winner: Team,
    /// Lines sent before the winner's line, in order.
    pub lines: Vec<Announcement>,
    /// Deaths the ending itself causes. Their lines are in `lines`.
    pub deaths: Vec<(PlayerId, KillMethod)>,
    /// Players auto-converted to the cult (Werewolf.cs:4583).
    pub converted: Vec<PlayerId>,
}

impl GameEnd {
    fn win(winner: Team) -> Self {
        GameEnd {
            winner,
            lines: vec![],
            deaths: vec![],
            converted: vec![],
        }
    }
}

const NEUTRAL: [Role; 4] = [Role::Sorcerer, Role::Tanner, Role::Thief, Role::Doppelganger];

/// `None` while the game goes on.
pub fn check_game_end(ctx: &EndContext, rng: &mut dyn RngSource) -> Option<GameEnd> {
    let alive = ctx.alive;
    let has = |role: Role| alive.iter().any(|p| p.role == role);
    let find = |role: Role| alive.iter().find(|p| p.role == role).map(|p| p.id);
    match alive.len() {
        0 => return Some(neutral_ending(ctx)),
        1 => {
            return Some(if NEUTRAL.contains(&alive[0].role) {
                neutral_ending(ctx)
            } else {
                GameEnd::win(alive[0].role.team())
            });
        }
        2 => {
            if alive.iter().all(|p| (ctx.in_love)(p.id)) {
                return Some(GameEnd::win(Team::Lovers));
            }
            if alive.iter().all(|p| NEUTRAL.contains(&p.role)) {
                return Some(neutral_ending(ctx));
            }
            if let Some(hunter) = find(Role::Hunter) {
                let other = alive.iter().find(|p| p.role != Role::Hunter);
                match other {
                    None => return Some(GameEnd::win(Team::Village)),
                    Some(sk) if sk.role == Role::SerialKiller => {
                        // Werewolf.cs:4880-4896: they take each other down.
                        return Some(GameEnd {
                            winner: Team::NoOne,
                            lines: vec![Announcement::SKHunterEnd {
                                serial_killer: sk.id,
                                hunter,
                            }],
                            deaths: vec![(sk.id, KillMethod::HunterShot), (hunter, KillMethod::SerialKilled)],
                            converted: vec![],
                        });
                    }
                    Some(wolf) if wolf.role.is_wolf() => {
                        return Some(if rng.chance(HUNTER_KILLS_WOLF_PERCENT) {
                            GameEnd {
                                winner: Team::Village,
                                lines: vec![Announcement::HunterKillsWolfEnd { hunter, wolf: wolf.id }],
                                deaths: vec![(wolf.id, KillMethod::HunterShot)],
                                converted: vec![],
                            }
                        } else {
                            GameEnd {
                                winner: Team::Wolf,
                                lines: vec![Announcement::WolfKillsHunterEnd { hunter, wolf: wolf.id }],
                                deaths: vec![(hunter, KillMethod::Eat)],
                                converted: vec![],
                            }
                        });
                    }
                    Some(_) => {}
                }
            }
            if let Some(sk) = find(Role::SerialKiller) {
                let other = alive.iter().find(|p| p.id != sk).expect("two alive").id;
                return Some(GameEnd {
                    winner: Team::SerialKiller,
                    lines: vec![Announcement::SerialKillerWinsOverpower {
                        serial_killer: sk,
                        other,
                    }],
                    deaths: vec![(other, KillMethod::SerialKilled)],
                    converted: vec![],
                });
            }
            let armed_gunner = alive.iter().any(|p| p.role == Role::Gunner && (ctx.bullets)(p.id) > 0);
            if let Some(arsonist) = find(Role::Arsonist) {
                if !armed_gunner {
                    let other = alive.iter().find(|p| p.id != arsonist).expect("two alive").id;
                    return Some(GameEnd {
                        winner: Team::Arsonist,
                        lines: vec![Announcement::ArsonistWinsOverpower { arsonist, other }],
                        deaths: vec![(other, KillMethod::Burn)],
                        converted: vec![],
                    });
                }
            }
            if has(Role::Cultist) {
                let Some(other) = alive.iter().find(|p| p.role != Role::Cultist) else {
                    return Some(GameEnd::win(Team::Cult));
                };
                let cultist = find(Role::Cultist).expect("checked above");
                return Some(match other.role {
                    r if r.is_wolf() => GameEnd::win(Team::Wolf),
                    Role::CultistHunter => GameEnd {
                        winner: Team::Village,
                        lines: vec![Announcement::CHKillsCultistEnd {
                            cultist,
                            hunter: other.id,
                        }],
                        deaths: vec![(cultist, KillMethod::Hunt)],
                        converted: vec![],
                    },
                    Role::Doppelganger | Role::Thief => GameEnd::win(Team::Cult),
                    _ => GameEnd {
                        converted: vec![other.id],
                        ..GameEnd::win(Team::Cult)
                    },
                });
            }
        }
        3 if alive
            .iter()
            .all(|p| matches!(p.role, Role::Sorcerer | Role::Thief | Role::Doppelganger)) =>
        {
            return Some(neutral_ending(ctx));
        }
        _ => {}
    }

    if alive
        .iter()
        .any(|p| matches!(p.role.team(), Team::SerialKiller | Team::Arsonist))
    {
        return None;
    }
    if alive.iter().all(|p| p.role.team() == Team::Cult) {
        return Some(GameEnd::win(Team::Cult));
    }
    let wolves: Vec<&AlivePlayer> = alive.iter().filter(|p| p.role.is_wolf()).collect();
    let others = alive.len() - wolves.len();
    if wolves.len() >= others {
        // Werewolf.cs:4612-4625: a Gunner with bullets can still turn it
        // around when the numbers are level, or when two of the wolves are
        // the lovers and one shot takes both.
        let armed_gunner = alive.iter().any(|p| p.role == Role::Gunner && (ctx.bullets)(p.id) > 0);
        let gunner_makes_the_difference = wolves.len() == others
            || (wolves.len() == others + 1 && wolves.iter().filter(|w| (ctx.in_love)(w.id)).count() == 2);
        if armed_gunner && gunner_makes_the_difference {
            return None;
        }
        return Some(GameEnd::win(Team::Wolf));
    }
    let no_baddies = alive
        .iter()
        .all(|p| !p.role.is_wolf() && !matches!(p.role, Role::Cultist | Role::SerialKiller | Role::Arsonist));
    if no_baddies && !alive.iter().any(|p| ctx.bitten.contains(&p.id)) {
        return Some(GameEnd::win(Team::Village));
    }
    None
}

/// `DoGameEnd(ITeam.NoOne)`'s closing lines (Werewolf.cs:4696-4806).
fn neutral_ending(ctx: &EndContext) -> GameEnd {
    let alive = ctx.alive;
    let find = |role: Role| alive.iter().find(|p| p.role == role).map(|p| p.id);
    let end = |player: PlayerId, role: Role| Announcement::NeutralEnd { player, role };
    let mut ending = GameEnd::win(Team::NoOne);
    match alive.len() {
        3 => {
            if let (Some(sorc), Some(dg), Some(thief)) =
                (find(Role::Sorcerer), find(Role::Doppelganger), find(Role::Thief))
            {
                ending.lines = vec![
                    end(sorc, Role::Sorcerer),
                    end(dg, Role::Doppelganger),
                    end(thief, Role::Thief),
                ];
            }
        }
        2 => {
            let tanner = find(Role::Tanner);
            let other = alive.iter().find(|p| Some(p.id) != tanner && NEUTRAL.contains(&p.role));
            match (tanner, other) {
                // The Tanner gives up first; a Doppelgänger whose role model
                // was the Tanner becomes one and follows (Werewolf.cs:4726-4746).
                (Some(tanner), Some(other)) => {
                    ending.deaths.push((tanner, KillMethod::Suicide));
                    ending.lines.push(end(tanner, Role::Tanner));
                    if other.role == Role::Doppelganger && (ctx.role_model)(other.id) == Some(tanner) {
                        ending.deaths.push((other.id, KillMethod::Suicide));
                        ending.lines.push(end(other.id, Role::Tanner));
                    } else {
                        ending.lines.push(end(other.id, other.role));
                    }
                }
                (None, _) => {
                    if let Some(sorc) = find(Role::Sorcerer) {
                        if let Some(other) = alive
                            .iter()
                            .find(|p| matches!(p.role, Role::Thief | Role::Doppelganger))
                        {
                            ending.lines = vec![end(sorc, Role::Sorcerer), end(other.id, other.role)];
                        }
                    } else if let (Some(thief), Some(dg)) = (find(Role::Thief), find(Role::Doppelganger)) {
                        ending.lines = vec![end(thief, Role::Thief), end(dg, Role::Doppelganger)];
                    }
                }
                _ => {}
            }
        }
        // A lone Tanner gets no line: legacy's lookup leaves the Tanner out
        // (Werewolf.cs:4789).
        1 => {
            let last = alive[0];
            if matches!(last.role, Role::Sorcerer | Role::Thief | Role::Doppelganger) {
                ending.lines = vec![end(last.id, last.role)];
            }
        }
        _ => {}
    }
    ending
}

/// `WolvesWin` rather than `WolfWins` when more than one wolf is left
/// (Werewolf.cs:4816). Legacy's condition also counts dead Snow Wolves
/// through an operator-precedence slip; `dead_snow_wolves` keeps that.
pub fn wolves_win_plural(alive: &[AlivePlayer], dead_snow_wolves: usize) -> bool {
    let living = alive.iter().filter(|p| p.role.is_wolf()).count();
    living + dead_snow_wolves > 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::ScriptedRngSource;

    fn roster(roles: &[Role]) -> Vec<AlivePlayer> {
        roles
            .iter()
            .enumerate()
            .map(|(i, &role)| AlivePlayer {
                id: PlayerId(i as u64 + 1),
                role,
            })
            .collect()
    }

    fn check_with(roles: &[Role], bullets: u8, rng: &mut ScriptedRngSource) -> Option<GameEnd> {
        let alive = roster(roles);
        let in_love = |_| false;
        let bullets = move |_| bullets;
        let role_model = |_| None;
        check_game_end(
            &EndContext {
                alive: &alive,
                in_love: &in_love,
                bullets: &bullets,
                role_model: &role_model,
                bitten: &[],
            },
            rng,
        )
    }

    fn check(roles: &[Role]) -> Option<GameEnd> {
        check_with(roles, 0, &mut ScriptedRngSource::new())
    }

    const A: PlayerId = PlayerId(1);
    const B: PlayerId = PlayerId(2);

    #[test]
    fn a_hunter_and_a_wolf_shoot_it_out() {
        let won = check_with(
            &[Role::Hunter, Role::Wolf],
            0,
            &mut ScriptedRngSource::new().with_chance(true),
        )
        .unwrap();
        assert_eq!(
            (won.winner, won.deaths),
            (Team::Village, vec![(B, KillMethod::HunterShot)])
        );
        let lost = check_with(
            &[Role::Hunter, Role::Wolf],
            0,
            &mut ScriptedRngSource::new().with_chance(false),
        )
        .unwrap();
        assert_eq!((lost.winner, lost.deaths), (Team::Wolf, vec![(A, KillMethod::Eat)]));
        assert_eq!(
            lost.lines,
            vec![Announcement::WolfKillsHunterEnd { hunter: A, wolf: B }]
        );
    }

    #[test]
    fn a_hunter_and_the_serial_killer_both_die() {
        let end = check(&[Role::Hunter, Role::SerialKiller]).unwrap();
        assert_eq!(end.winner, Team::NoOne);
        assert_eq!(end.deaths.len(), 2);
        assert_eq!(check(&[Role::Hunter, Role::Hunter]).unwrap().winner, Team::Village);
    }

    #[test]
    fn the_serial_killer_overpowers_the_last_one() {
        let end = check(&[Role::Villager, Role::SerialKiller]).unwrap();
        assert_eq!(
            (end.winner, end.deaths),
            (Team::SerialKiller, vec![(A, KillMethod::SerialKilled)])
        );
    }

    #[test]
    fn the_arsonist_wins_unless_a_gunner_can_still_shoot() {
        assert_eq!(check(&[Role::Gunner, Role::Arsonist]).unwrap().winner, Team::Arsonist);
        assert_eq!(
            check_with(&[Role::Gunner, Role::Arsonist], 1, &mut ScriptedRngSource::new()),
            None
        );
    }

    #[test]
    fn the_cult_endings() {
        assert_eq!(check(&[Role::Cultist, Role::Cultist]).unwrap().winner, Team::Cult);
        assert_eq!(check(&[Role::Cultist, Role::Wolf]).unwrap().winner, Team::Wolf);
        let ch = check(&[Role::Cultist, Role::CultistHunter]).unwrap();
        assert_eq!((ch.winner, ch.deaths), (Team::Village, vec![(A, KillMethod::Hunt)]));
        let convert = check(&[Role::Cultist, Role::Villager]).unwrap();
        assert_eq!((convert.winner, convert.converted), (Team::Cult, vec![B]));
        assert_eq!(check(&[Role::Cultist, Role::Thief]).unwrap().converted, vec![]);
    }

    #[test]
    fn a_gunner_with_bullets_holds_off_level_wolves() {
        let roles = [Role::Wolf, Role::Wolf, Role::Gunner, Role::Villager];
        assert_eq!(check_with(&roles, 1, &mut ScriptedRngSource::new()), None);
        assert_eq!(check(&roles).unwrap().winner, Team::Wolf);
        let outnumbered = [Role::Wolf, Role::Wolf, Role::Wolf, Role::Gunner, Role::Villager];
        assert_eq!(
            check_with(&outnumbered, 1, &mut ScriptedRngSource::new())
                .unwrap()
                .winner,
            Team::Wolf
        );
    }

    #[test]
    fn neutral_endings_name_who_was_left() {
        let tanner = check(&[Role::Tanner, Role::Sorcerer]).unwrap();
        assert_eq!(tanner.deaths, vec![(A, KillMethod::Suicide)]);
        assert_eq!(
            tanner.lines,
            vec![
                Announcement::NeutralEnd {
                    player: A,
                    role: Role::Tanner
                },
                Announcement::NeutralEnd {
                    player: B,
                    role: Role::Sorcerer
                }
            ]
        );
        assert_eq!(check(&[Role::Tanner]).unwrap().lines, vec![]);
        assert_eq!(
            check(&[Role::Thief]).unwrap().lines,
            vec![Announcement::NeutralEnd {
                player: A,
                role: Role::Thief
            }]
        );
    }

    #[test]
    fn the_village_waits_for_a_bitten_player_to_turn() {
        let alive = roster(&[Role::Villager, Role::Seer]);
        let (in_love, bullets, role_model) = (|_| false, |_| 0, |_| None);
        let ctx = EndContext {
            alive: &alive,
            in_love: &in_love,
            bullets: &bullets,
            role_model: &role_model,
            bitten: &[A],
        };
        assert_eq!(check_game_end(&ctx, &mut ScriptedRngSource::new()), None);
    }
}
