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
}

/// The line for one death, or `None` when another line already covers it
/// (the Spumpkin's own death is part of its detonation).
pub fn death_message(d: &DeathContext) -> Option<Message> {
    use MessageArg::{Empty, Player};
    let v = Player(d.victim);
    let reveal = if d.reveal { MessageArg::Reveal { player: d.victim, role: d.role } } else { Empty };
    let message = match (d.method, d.by) {
        (KillMethod::Lynch, _) => Message::new("LynchKill", vec![v, reveal]),
        // Werewolf.cs:848-856.
        (KillMethod::Flee, _) => {
            let flee = Message::new("Flee", vec![v]);
            if d.reveal {
                flee.followed_by(Message::new("PlayerRoleWas", vec![Player(d.victim), MessageArg::Role(d.role)]))
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
            let key = if d.killer_was_lynched { "HunterKilledFinalLynched" } else { "HunterKilledFinalShot" };
            Message::new(key, vec![Player(hunter), v, reveal])
        }
        // A wolf or Snow Wolf the Hunter took down at night
        // (Werewolf.cs:4336, 4393). The role is always named here.
        (KillMethod::HunterShot, None) if d.role == Role::SnowWolf => Message::new("SnowFrozeHunter", vec![v]),
        (KillMethod::HunterShot, None) => {
            let key = if d.pack_size > 1 { "HunterShotWolfMulti" } else { "HunterShotWolf" };
            Message::new(key, vec![v])
                .followed_by(Message::new("PlayerRoleWas", vec![Player(d.victim), MessageArg::Role(d.role)]))
        }
        // Werewolf.cs:4348: the cult's recruit shot by the Hunter.
        (KillMethod::HunterCult, _) => Message::new("HunterKilledVisiter", vec![v, MessageArg::RoleIsDead(d.role)]),
        // Werewolf.cs:5711.
        (KillMethod::LoverDied, Some((first, _))) => Message::new("LoverDied", vec![Player(first), v, reveal]),
        // Werewolf.cs:4217-4220: with roles hidden, every other death is
        // announced the same way.
        _ if !d.reveal => Message::new("GenericDeathNoReveal", vec![v]),
        _ => match death_locale_key(d.role, d.method) {
            Some(key) => Message::new(key, vec![v, reveal]),
            // Deaths whose own legacy lines aren't ported yet (burns,
            // poison, some visit deaths) still get announced.
            None => Message::new("GenericDeathNoReveal", vec![v])
                .followed_by(Message::new("PlayerRoleWas", vec![Player(d.victim), MessageArg::Role(d.role)])),
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
        DeathContext { victim: V, role, method, by, killer_was_lynched: false, reveal, pack_size: 1 }
    }

    fn key(d: DeathContext) -> &'static str {
        death_message(&d).unwrap().key
    }

    #[test]
    fn a_lynch_reveals_the_role_only_when_shown() {
        let shown = death_message(&death(Role::Seer, KillMethod::Lynch, None, true)).unwrap();
        assert_eq!(shown.args, vec![MessageArg::Player(V), MessageArg::Reveal { player: V, role: Role::Seer }]);
        let hidden = death_message(&death(Role::Seer, KillMethod::Lynch, None, false)).unwrap();
        assert_eq!(hidden.args, vec![MessageArg::Player(V), MessageArg::Empty]);
    }

    #[test]
    fn night_deaths_are_generic_when_roles_are_hidden() {
        assert_eq!(key(death(Role::Seer, KillMethod::Eat, None, true)), "SeerEaten");
        assert_eq!(key(death(Role::Seer, KillMethod::Eat, None, false)), "GenericDeathNoReveal");
    }

    #[test]
    fn shots_name_the_shooter() {
        let shot = death_message(&death(Role::Villager, KillMethod::Shoot, Some((K, Role::Gunner)), false)).unwrap();
        assert_eq!((shot.key, shot.args[0].clone()), ("DefaultShot", MessageArg::Player(K)));
        assert_eq!(key(death(Role::Harlot, KillMethod::Shoot, Some((K, Role::Gunner)), true)), "HarlotShot");
        assert_eq!(key(death(Role::Harlot, KillMethod::Shoot, Some((K, Role::Gunner)), false)), "DefaultShot");
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
        let m = death_message(&death(Role::Wolf, KillMethod::HunterShot, None, false)).unwrap();
        assert_eq!(m.key, "HunterShotWolf");
        assert_eq!(m.then.unwrap().key, "PlayerRoleWas");
        assert_eq!(key(death(Role::SnowWolf, KillMethod::HunterShot, None, false)), "SnowFrozeHunter");
    }

    #[test]
    fn a_lover_names_who_they_followed() {
        let m = death_message(&death(Role::Villager, KillMethod::LoverDied, Some((K, Role::Seer)), false)).unwrap();
        assert_eq!((m.key, m.args[0].clone(), m.args[1].clone()), ("LoverDied", MessageArg::Player(K), MessageArg::Player(V)));
    }

    #[test]
    fn a_death_without_its_own_line_is_still_announced() {
        let m = death_message(&death(Role::Villager, KillMethod::Poison, None, true)).unwrap();
        assert_eq!((m.key, m.then.unwrap().key), ("GenericDeathNoReveal", "PlayerRoleWas"));
    }

    #[test]
    fn the_spumpkin_dies_inside_its_own_detonation_line() {
        assert_eq!(death_message(&death(Role::Spumpkin, KillMethod::Shoot, None, true)), None);
        assert_eq!(key(death(Role::Villager, KillMethod::Shoot, Some((K, Role::Spumpkin)), true)), "Detonation");
    }
}
