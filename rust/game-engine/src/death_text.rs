//! The public line each death gets, picked the way the legacy game picks
//! it: by how the player died, who did it, their role, and whether the
//! group shows roles on death. The engine attaches the result to
//! `NarrationEvent::Death`, so every presenter sends the same line.

use crate::messages::{Message, MessageArg};
use crate::orchestrator::death_locale_key;
use crate::roles::PlayerId;
use shared::{KillMethod, Role};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeathContext {
    pub victim: PlayerId,
    pub role: Role,
    pub method: KillMethod,
    /// Whoever caused the death, with their role: the Gunner or Spumpkin
    /// for a day shot, the Hunter for a final shot, the first lover for a
    /// lover's death.
    pub by: Option<(PlayerId, Role)>,
    /// For a Hunter's final shot: whether the Hunter was lynched.
    pub killer_was_lynched: bool,
    /// `ShowRolesDeath`.
    pub reveal: bool,
    /// Living wolves when a Hunter shot one of them at night.
    pub pack_size: usize,
    /// The legacy `KilledByRole`: the Serial Killer or the wolves, for a
    /// Harlot who visited their victim or a spotted Grave Digger.
    pub killed_by: Option<Role>,
}

/// The line for one death, or `None` when another line already covers it
/// (the Spumpkin's own death is part of its detonation).
pub fn death_message(d: &DeathContext) -> Option<Message> {
    use MessageArg::{Empty, Player};
    let v = Player(d.victim);
    let reveal = if d.reveal {
        MessageArg::Reveal {
            player: d.victim,
            role: d.role,
        }
    } else {
        Empty
    };
    let message = match (d.method, d.by) {
        (KillMethod::Lynch, _) => Message::new("LynchKill", vec![v, reveal]),
        // Werewolf.cs:848-856.
        (KillMethod::Flee, _) => {
            let flee = Message::new("Flee", vec![v]);
            if d.reveal {
                flee.followed_by(Message::new(
                    "PlayerRoleWas",
                    vec![Player(d.victim), MessageArg::Role(d.role)],
                ))
            } else {
                flee
            }
        }
        // Werewolf.cs:2914. "Detonation" is missing from English.xml (the
        // Spumpkin's lines only exist in other language files), so it
        // renders as a visible gap in English.
        (KillMethod::Shoot, Some((spumpkin, Role::Spumpkin))) => {
            Message::new("Detonation", vec![Player(spumpkin), v, reveal])
        }
        (KillMethod::Shoot, None) if d.role == Role::Spumpkin => return None,
        // Werewolf.cs:2884-2894.
        (KillMethod::Shoot, Some((gunner, _))) if d.role == Role::Harlot && d.reveal => {
            Message::new("HarlotShot", vec![Player(gunner), v])
        }
        (KillMethod::Shoot, Some((gunner, _))) => Message::new("DefaultShot", vec![Player(gunner), v, reveal]),
        // Werewolf.cs:5470.
        (KillMethod::HunterShot, Some((hunter, _))) => {
            let key = if d.killer_was_lynched {
                "HunterKilledFinalLynched"
            } else {
                "HunterKilledFinalShot"
            };
            Message::new(key, vec![Player(hunter), v, reveal])
        }
        // Werewolf.cs:5711.
        (KillMethod::LoverDied, Some((first, _))) => Message::new("LoverDied", vec![Player(first), v, reveal]),
        // Werewolf.cs:4217-4220: with roles hidden, every other death is
        // announced the same way.
        _ if !d.reveal => Message::new("GenericDeathNoReveal", vec![v]),
        // A wolf or Snow Wolf the Hunter took down at night
        // (Werewolf.cs:4336, 4393).
        (KillMethod::HunterShot, None) if d.role == Role::SnowWolf => Message::new("SnowFrozeHunter", vec![v]),
        (KillMethod::HunterShot, None) => {
            let key = if d.pack_size > 1 {
                "HunterShotWolfMulti"
            } else {
                "HunterShotWolf"
            };
            Message::new(key, vec![v]).followed_by(Message::new(
                "PlayerRoleWas",
                vec![Player(d.victim), MessageArg::Role(d.role)],
            ))
        }
        // Werewolf.cs:4348: the cult's recruit shot by the Hunter.
        (KillMethod::HunterCult, _) => Message::new("HunterKilledVisiter", vec![v, MessageArg::RoleIsDead(d.role)]),
        // Werewolf.cs:4364-4371: the Harlot and the host she found dead.
        (KillMethod::VisitVictim, Some((host, _))) => {
            let key = if d.killed_by == Some(Role::SerialKiller) {
                "HarlotFuckedKilledPublic"
            } else {
                "HarlotFuckedVictimPublic"
            };
            Message::new(key, vec![v, Player(host)])
        }
        // Werewolf.cs:4399-4403.
        (KillMethod::Spotted, _) => {
            let key = if d.killed_by == Some(Role::SerialKiller) {
                "KillerSpottedDiggerPublic"
            } else {
                "WolvesSpottedDiggerPublic"
            };
            Message::new(key, vec![v])
        }
        // Werewolf.cs:4305-4320, walking into a burning house.
        (KillMethod::VisitBurning, _) => {
            let wolf = d.role.is_eating_wolf();
            Message::new(if wolf { "WolfVisitBurn" } else { "DefaultVisitBurn" }, vec![v, reveal])
        }
        // Werewolf.cs:4376.
        (KillMethod::GuardWolf, _) => Message::new("GAGuardedWolf", vec![v]),
        // Werewolf.cs:4352.
        (KillMethod::VisitWolf, _) if d.role == Role::Cultist => Message::new("CultConvertWolfPublic", vec![v]),
        // Werewolf.cs:4276-4281: the Chemist loses their role for it.
        (KillMethod::Chemistry, Some((chemist, _))) if d.role == Role::WiseElder && chemist != d.victim => {
            Message::new("ChemistSuccessPublic", vec![v, reveal])
                .followed_by(Message::new("ChemistKillWiseElder", vec![]))
        }
        _ => match death_locale_key(d.role, d.method) {
            Some(key) => Message::new(key, vec![v, reveal]),
            // Deaths whose own legacy lines aren't ported yet (burns,
            // poison, some visit deaths) still get announced.
            None => Message::new("GenericDeathNoReveal", vec![v]).followed_by(Message::new(
                "PlayerRoleWas",
                vec![Player(d.victim), MessageArg::Role(d.role)],
            )),
        },
    };
    Some(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    const V: PlayerId = PlayerId(1);
    const K: PlayerId = PlayerId(2);

    fn death(role: Role, method: KillMethod, by: Option<(PlayerId, Role)>, reveal: bool) -> DeathContext {
        DeathContext {
            victim: V,
            role,
            method,
            by,
            killer_was_lynched: false,
            reveal,
            pack_size: 1,
            killed_by: None,
        }
    }

    fn key(d: DeathContext) -> &'static str {
        death_message(&d).unwrap().key
    }

    #[test]
    fn a_lynch_reveals_the_role_only_when_shown() {
        let shown = death_message(&death(Role::Seer, KillMethod::Lynch, None, true)).unwrap();
        assert_eq!(
            shown.args,
            vec![
                MessageArg::Player(V),
                MessageArg::Reveal {
                    player: V,
                    role: Role::Seer
                }
            ]
        );
        let hidden = death_message(&death(Role::Seer, KillMethod::Lynch, None, false)).unwrap();
        assert_eq!(hidden.args, vec![MessageArg::Player(V), MessageArg::Empty]);
    }

    #[test]
    fn night_deaths_are_generic_when_roles_are_hidden() {
        assert_eq!(key(death(Role::Seer, KillMethod::Eat, None, true)), "SeerEaten");
        assert_eq!(
            key(death(Role::Seer, KillMethod::Eat, None, false)),
            "GenericDeathNoReveal"
        );
    }

    #[test]
    fn shots_name_the_shooter() {
        let shot = death_message(&death(
            Role::Villager,
            KillMethod::Shoot,
            Some((K, Role::Gunner)),
            false,
        ))
        .unwrap();
        assert_eq!((shot.key, shot.args[0].clone()), ("DefaultShot", MessageArg::Player(K)));
        assert_eq!(
            key(death(Role::Harlot, KillMethod::Shoot, Some((K, Role::Gunner)), true)),
            "HarlotShot"
        );
        assert_eq!(
            key(death(Role::Harlot, KillMethod::Shoot, Some((K, Role::Gunner)), false)),
            "DefaultShot"
        );
    }

    #[test]
    fn a_hunter_final_shot_depends_on_how_the_hunter_died() {
        let mut d = death(Role::Villager, KillMethod::HunterShot, Some((K, Role::Hunter)), true);
        assert_eq!(key(d), "HunterKilledFinalShot");
        d.killer_was_lynched = true;
        assert_eq!(key(d), "HunterKilledFinalLynched");
    }

    #[test]
    fn a_hunter_shooting_a_wolf_at_night_names_its_role() {
        let m = death_message(&death(Role::Wolf, KillMethod::HunterShot, None, true)).unwrap();
        assert_eq!(m.key, "HunterShotWolf");
        assert_eq!(m.then.unwrap().key, "PlayerRoleWas");
        assert_eq!(
            key(death(Role::SnowWolf, KillMethod::HunterShot, None, true)),
            "SnowFrozeHunter"
        );
        assert_eq!(
            key(death(Role::Wolf, KillMethod::HunterShot, None, false)),
            "GenericDeathNoReveal"
        );
    }

    #[test]
    fn a_lover_names_who_they_followed() {
        let m = death_message(&death(
            Role::Villager,
            KillMethod::LoverDied,
            Some((K, Role::Seer)),
            false,
        ))
        .unwrap();
        assert_eq!(
            (m.key, m.args[0].clone(), m.args[1].clone()),
            ("LoverDied", MessageArg::Player(K), MessageArg::Player(V))
        );
    }

    #[test]
    fn a_death_without_its_own_line_is_still_announced() {
        let m = death_message(&death(Role::Villager, KillMethod::Poison, None, true)).unwrap();
        assert_eq!((m.key, m.then.unwrap().key), ("GenericDeathNoReveal", "PlayerRoleWas"));
    }

    #[test]
    fn a_harlot_who_found_her_host_dead_names_them() {
        let mut d = death(Role::Harlot, KillMethod::VisitVictim, Some((K, Role::Villager)), true);
        d.killed_by = Some(Role::Wolf);
        let m = death_message(&d).unwrap();
        assert_eq!(
            (m.key, m.args),
            (
                "HarlotFuckedVictimPublic",
                vec![MessageArg::Player(V), MessageArg::Player(K)]
            )
        );
        d.killed_by = Some(Role::SerialKiller);
        assert_eq!(key(d), "HarlotFuckedKilledPublic");
    }

    #[test]
    fn a_spotted_grave_digger_says_who_spotted_them() {
        let mut d = death(
            Role::GraveDigger,
            KillMethod::Spotted,
            Some((K, Role::SerialKiller)),
            true,
        );
        d.killed_by = Some(Role::SerialKiller);
        assert_eq!(key(d), "KillerSpottedDiggerPublic");
        d.killed_by = Some(Role::Wolf);
        assert_eq!(key(d), "WolvesSpottedDiggerPublic");
    }

    #[test]
    fn visiting_deaths_have_their_own_lines() {
        assert_eq!(
            key(death(Role::Wolf, KillMethod::VisitBurning, None, true)),
            "WolfVisitBurn"
        );
        assert_eq!(
            key(death(Role::Harlot, KillMethod::VisitBurning, None, true)),
            "DefaultVisitBurn"
        );
        assert_eq!(
            key(death(Role::GuardianAngel, KillMethod::GuardWolf, None, true)),
            "GAGuardedWolf"
        );
        assert_eq!(
            key(death(Role::GuardianAngel, KillMethod::VisitKiller, None, true)),
            "GAGuardedKiller"
        );
        assert_eq!(
            key(death(Role::Cultist, KillMethod::VisitWolf, None, true)),
            "CultConvertWolfPublic"
        );
        assert_eq!(
            key(death(Role::Chemist, KillMethod::VisitKiller, None, true)),
            "ChemistSKPublic"
        );
    }

    #[test]
    fn the_chemist_killing_the_wise_elder_is_announced() {
        let m = death_message(&death(
            Role::WiseElder,
            KillMethod::Chemistry,
            Some((K, Role::Chemist)),
            true,
        ))
        .unwrap();
        assert_eq!(
            (m.key, m.then.unwrap().key),
            ("ChemistSuccessPublic", "ChemistKillWiseElder")
        );
    }

    #[test]
    fn the_spumpkin_dies_inside_its_own_detonation_line() {
        assert_eq!(
            death_message(&death(Role::Spumpkin, KillMethod::Shoot, None, true)),
            None
        );
        assert_eq!(
            key(death(
                Role::Villager,
                KillMethod::Shoot,
                Some((K, Role::Spumpkin)),
                true
            )),
            "Detonation"
        );
    }
}
