//! The legacy locale keys for deaths and game endings, and the list of
//! every key the engine can produce.

use super::{prompt_locale_key, ALL_PROMPTS};
use crate::roles::def;
use shared::{KillMethod, Role, Team};

/// Maps a death to the legacy flavor key, where there is one.
///
/// The legacy `*Killed` keys (`GunnerKilled`, `SeerKilled`, `PrinceKilled`,
/// ...) are all Serial-Killer-specific flavor text, not general per-role
/// death messages (confirmed by reading their actual English.xml values —
/// every one mentions "the serial killer has struck again"), so they're
/// only used for `KillMethod::SerialKilled` specifically, never for a
/// lynch/eat/etc. death of the same role — `DefaultKilled` covers every
/// role without its own dedicated key. `KillMethod::VisitWolf` (a Harlot
/// visiting the wolves' actual target, see the Harlot region of `night`)
/// gets the real public death text too (`HarlotFuckedWolfPublic` — yes,
/// that's the actual legacy key name).
///
/// `KillMethod::LoverDied`/`HunterShot` are still `None`: their real text
/// needs *two* names (the lover who died first and the one dying of
/// grief; the shooter and the target) plus a role reveal, and
/// `NarrationEvent::Death` only carries one victim — giving them real
/// flavor text needs that event to grow a second identity field first,
/// which is more than this batch of "easy, unambiguous" mappings signed
/// up for.
///
/// The lines that depend on the victim's role (eaten, killed by the Serial
/// Killer, fallen into a grave, killed visiting the Serial Killer) are the
/// role's `RoleDef::deaths`. `KillMethod::Eat` covers the wolves' kill
/// and the Wolf Cub's bonus kill alike (Werewolf.cs:4218-4267).
pub fn death_locale_key(role: Role, method: KillMethod) -> Option<&'static str> {
    match method {
        KillMethod::Lynch => Some("LynchKill"),
        // The Harlot dying by visiting the wolves' actual target
        // (Werewolf.cs:2359-2360, `killedByRole: IRole.Wolf`). Matched to
        // her own case in the "died by visiting" switch
        // (Werewolf.cs:4358-4364: `DiedByVisitingKiller` + `KilledByRole
        // == Wolf` => `"HarlotFuckedWolfPublic"`) — not
        // `"HarlotFuckKillerPublic"`, which is the *SerialKiller*-visited
        // variant of the same switch (Werewolf.cs:4368-4369).
        KillMethod::VisitWolf => Some("HarlotFuckedWolfPublic"),
        // Whoever dies visiting the Serial Killer player themself
        // (Werewolf.cs:2339-2347's shared `VisitPlayer` gate, reachable
        // for three different visitors this port now models: a Harlot who
        // picked the SK as her target, a wolf who drew the short straw in
        // the wolves region of `night`, or the cult's
        // newbie in the cult region of `night`). Genuinely three
        // different real messages depending on the dying role — the
        // "died by visiting" switch (Werewolf.cs:4322-4406) branches on
        // `p.PlayerRole` before it ever looks at `KilledByRole`, so a
        // wolf and a Harlot dying to the exact same mechanic still get
        // different text.
        // Werewolf.cs:4322-4406: the line depends on who walked in.
        KillMethod::VisitKiller => def(role).deaths.visited_serial_killer,
        // A Cultist killed by the Cultist Hunter, hunted or while trying
        // to convert them (Werewolf.cs:4342-4346).
        KillMethod::Hunt => Some("HunterKilledCultist"),
        // The Harlot visiting whoever the Serial Killer separately killed
        // that same night — a different player from the SK themself
        // (Werewolf.cs:3893-3897 → `KilledByRole == SerialKiller` +
        // `DiedByVisitingVictim`, 4370-4371).
        KillMethod::VisitVictim => Some("HarlotFuckedKilledPublic"),
        KillMethod::Eat => Some(def(role).deaths.eaten),
        // Werewolf.cs:4251-4262.
        KillMethod::SerialKilled => Some(def(role).deaths.serial_killed),
        // Werewolf.cs:4288-4302, the "killed by visiting Grave Digger"
        // switch.
        KillMethod::FallGrave => Some(def(role).deaths.fell_in_grave),
        // The Chemist region of `night` produces this for either the target (success)
        // or the Chemist themself (failure) - two different real messages
        // (Werewolf.cs:3801-3814: `ChemistSuccessPublic`/`ChemistFailPublic`),
        // each needing only the one name involved (unlike `HunterShot`/
        // `LoverDied`, which need two and stay unmapped for exactly that
        // reason). `role == Chemist` is what tells the two apart here: if
        // the Chemist is the one who died, this had to be the failure
        // case, not a coincidence of some *other* dead Chemist - only one
        // Chemist exists per game, the same uniqueness every other
        // single-instance role in this switch already relies on.
        KillMethod::Chemistry => Some(if role == Role::Chemist {
            "ChemistFailPublic"
        } else {
            "ChemistSuccessPublic"
        }),
        // Werewolf.cs:848: `SendWithQueue(GetLocaleString("Flee", p
        // .GetName()))`, sent unconditionally the moment a mid-game flee
        // resolves - real, role-agnostic, single-name text, unlike most
        // of this switch.
        KillMethod::Flee => Some("Flee"),
        _ => None,
    }
}

/// Maps a game-ending `WinOutcome` to its real legacy per-team victory
/// key. Only defined for an actual `Team` win — the RNG/data-gap
/// `WinOutcome` variants (`RngDependent`, `InsufficientData`,
/// `Unimplemented`) have no legacy equivalent to look up, since the real
/// game always resolved to a concrete winner one way or another; that gap
/// is exactly what those variants exist to flag (see `WinOutcome`'s own
/// doc). `Team::Thief` has no key either: it is
/// the Thief's transient pre-steal team, never itself a winning faction
/// in `evaluate_winner_with_kills` (checked: nothing produces
/// `WinOutcome::Team(Team::Thief)` today). `Team::NoOne` is `NoWinner`
/// (Werewolf.cs:4810). `Thief` is still named explicitly
/// rather than folded behind the RNG/data-gap arm below, so a reader
/// isn't left wondering whether their absence is an oversight.
pub fn game_over_locale_key(outcome: &crate::WinOutcome) -> Option<&'static str> {
    use crate::WinOutcome;
    match outcome {
        WinOutcome::Team(Team::Village) => Some("VillageWins"),
        WinOutcome::Team(Team::Wolf) => Some("WolfWins"),
        WinOutcome::Team(Team::Tanner) => Some("TannerWins"),
        WinOutcome::Team(Team::Cult) => Some("CultWins"),
        WinOutcome::Team(Team::SerialKiller) => Some("SerialKillerWins"),
        WinOutcome::Team(Team::Arsonist) => Some("ArsonistWins"),
        WinOutcome::Team(Team::Lovers) => Some("LoversWin"),
        WinOutcome::Team(Team::NoOne) => Some("NoWinner"),
        WinOutcome::Team(Team::Thief)
        | WinOutcome::RngDependent(_)
        | WinOutcome::InsufficientData(_)
        | WinOutcome::Unimplemented(_)
        | WinOutcome::Killed => None,
    }
}

/// The winner's line: `game_over_locale_key`, except that more than one
/// wolf left makes it `WolvesWin` (Werewolf.cs:4816-4825).
pub fn game_over_message(outcome: &crate::WinOutcome, several_wolves: bool) -> Option<crate::messages::Message> {
    let key = match outcome {
        crate::WinOutcome::Team(Team::Wolf) if several_wolves => "WolvesWin",
        _ => game_over_locale_key(outcome)?,
    };
    Some(crate::messages::Message::new(key, vec![]))
}

/// Every distinct locale key `prompt_locale_key`/`death_locale_key` can
/// currently produce, in one place — the input to the locale-coverage
/// completeness check (`sim/tests/locale_coverage.rs`), which measures
/// "how much of English.xml does this port actually reach" as a real
/// number instead of a vibe. Grows every time a mapping table grows;
/// nothing computes this automatically today (there's no exhaustive
/// `Role`/`KillMethod` iterator to drive `death_locale_key` with yet, the
/// same "two hardcoded levels are enough until a second case shows up"
/// tradeoff as the night's two-batch dependency resolution), so this is
/// hand-kept in sync rather than derived — small enough today that a
/// missed update would be obvious in the coverage number itself.
pub fn all_mapped_locale_keys() -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = ALL_PROMPTS.iter().filter_map(|&p| prompt_locale_key(p)).collect();
    keys.extend([
        // death_locale_key's non-`None` outputs.
        "LynchKill",
        "HarlotFuckedWolfPublic",
        "HarlotFuckKillerPublic",
        "HarlotFuckedKilledPublic",
        "SerialKillerKilledWolf",
        "SnowFrozeKiller",
        "CultConvertKillerPublic",
        "SerialKillerKilledCH",
        "ThiefStoleKiller",
        "HunterKilledCultist",
        // death_text's lines beyond death_locale_key.
        "Was",
        "IsDead",
        "PlayerRoleWas",
        "GenericDeathNoReveal",
        "DefaultShot",
        "HarlotShot",
        "HunterKilledFinalShot",
        "HunterKilledFinalLynched",
        "HunterShotWolf",
        "HunterShotWolfMulti",
        "SnowFrozeHunter",
        "HunterKilledVisiter",
        "LoverDied",
        "ApprenticeSeerEaten",
        "DetectiveEaten",
        "DrunkEaten",
        "FoolEaten",
        "GunnerEaten",
        "HarlotEaten",
        "MasonEaten",
        "SeerEaten",
        "SorcererEaten",
        "WildChildEaten",
        "GuardianEaten",
        "DefaultEaten",
        "GunnerKilled",
        "SeerKilled",
        "DrunkKilled",
        "MayorKilled",
        "PrinceKilled",
        "CultistKilled",
        "GuardianAngelKilled",
        "BlacksmithKilled",
        "CupidKilled",
        "DefaultKilled",
        "HarlotFellPublic",
        "DefaultFellPublic",
        "WolfFellPublic",
        "CultistFellPublic",
        "ThiefFellPublic",
        // death_text's night lines.
        "GAGuardedKiller",
        "ChemistSKPublic",
        "ArsonistVisitKiller",
        "WolfVisitBurn",
        "DefaultVisitBurn",
        "GAGuardedWolf",
        "CultConvertWolfPublic",
        "HarlotFuckedVictimPublic",
        "KillerSpottedDiggerPublic",
        "WolvesSpottedDiggerPublic",
        "ChemistSuccessPublic",
        "ChemistFailPublic",
        "Flee",
        // game_over_locale_key's non-`None` outputs.
        "VillageWins",
        "WolfWins",
        "TannerWins",
        "CultWins",
        "SerialKillerWins",
        "ArsonistWins",
        "LoversWin",
        "NoWinner",
        "WolvesWin",
    ]);
    keys.extend(crate::messages::all_message_keys());
    // Role display names, which notes like `SeerSees` fill in. The four
    // left out have no English.xml entry (see `Role::locale_key`).
    keys.extend(
        shared::ALL_ROLES
            .iter()
            .filter(|r| !matches!(r, Role::Spumpkin | Role::Chef | Role::Barkeep | Role::Witch))
            .map(|r| r.locale_key()),
    );
    // Every key a role's `RoleDef` names, so a new role's text is checked
    // the day it is added. Keys with a fallback text are not in English.xml.
    for &role in shared::ALL_ROLES {
        let d = def(role);
        let deaths = [d.deaths.eaten, d.deaths.serial_killed, d.deaths.fell_in_grave];
        let role_keys = [d.info_key]
            .into_iter()
            .chain(deaths)
            .chain(d.deaths.visited_serial_killer);
        keys.extend(role_keys.filter(|k| crate::messages::fallback_text(k).is_none()));
    }
    keys.sort_unstable();
    keys.dedup();
    keys
}

#[cfg(test)]
mod locale_mapping_tests {
    use super::*;
    use crate::WinOutcome;

    #[test]
    fn serial_killed_maps_to_the_dying_roles_own_flavor_key() {
        assert_eq!(
            death_locale_key(Role::Gunner, KillMethod::SerialKilled),
            Some("GunnerKilled")
        );
        assert_eq!(
            death_locale_key(Role::Seer, KillMethod::SerialKilled),
            Some("SeerKilled")
        );
        assert_eq!(
            death_locale_key(Role::Blacksmith, KillMethod::SerialKilled),
            Some("BlacksmithKilled")
        );
        assert_eq!(
            death_locale_key(Role::Cupid, KillMethod::SerialKilled),
            Some("CupidKilled")
        );
    }

    #[test]
    fn serial_killed_falls_back_to_default_for_an_uncelebrated_role() {
        assert_eq!(
            death_locale_key(Role::Villager, KillMethod::SerialKilled),
            Some("DefaultKilled")
        );
    }

    #[test]
    fn a_harlot_dying_from_visiting_a_wolf_has_a_key() {
        assert_eq!(
            death_locale_key(Role::Harlot, KillMethod::VisitWolf),
            Some("HarlotFuckedWolfPublic")
        );
    }

    #[test]
    fn wolf_eat_maps_to_the_dying_roles_own_flavor_key() {
        assert_eq!(death_locale_key(Role::Seer, KillMethod::Eat), Some("SeerEaten"));
        assert_eq!(
            death_locale_key(Role::Detective, KillMethod::Eat),
            Some("DetectiveEaten")
        );
        assert_eq!(
            death_locale_key(Role::GuardianAngel, KillMethod::Eat),
            Some("GuardianEaten")
        );
    }

    #[test]
    fn wolf_eat_falls_back_to_default_for_an_uncelebrated_role() {
        assert_eq!(death_locale_key(Role::Villager, KillMethod::Eat), Some("DefaultEaten"));
    }

    #[test]
    fn chemistry_success_and_failure_map_to_different_keys() {
        assert_eq!(
            death_locale_key(Role::Villager, KillMethod::Chemistry),
            Some("ChemistSuccessPublic")
        );
        assert_eq!(
            death_locale_key(Role::Chemist, KillMethod::Chemistry),
            Some("ChemistFailPublic")
        );
    }

    #[test]
    fn visit_killer_deaths_vary_by_the_dying_roles_own_team() {
        assert_eq!(
            death_locale_key(Role::Wolf, KillMethod::VisitKiller),
            Some("SerialKillerKilledWolf")
        );
        assert_eq!(
            death_locale_key(Role::AlphaWolf, KillMethod::VisitKiller),
            Some("SerialKillerKilledWolf")
        );
        assert_eq!(
            death_locale_key(Role::SnowWolf, KillMethod::VisitKiller),
            Some("SnowFrozeKiller")
        );
        assert_eq!(
            death_locale_key(Role::Cultist, KillMethod::VisitKiller),
            Some("CultConvertKillerPublic")
        );
        assert_eq!(
            death_locale_key(Role::Harlot, KillMethod::VisitKiller),
            Some("HarlotFuckKillerPublic")
        );
        assert_eq!(death_locale_key(Role::Villager, KillMethod::VisitKiller), None);
    }

    #[test]
    fn flee_has_one_role_agnostic_key() {
        assert_eq!(death_locale_key(Role::Villager, KillMethod::Flee), Some("Flee"));
        assert_eq!(death_locale_key(Role::Seer, KillMethod::Flee), Some("Flee"));
    }

    #[test]
    fn lover_died_and_hunter_shot_still_have_no_key() {
        assert_eq!(death_locale_key(Role::Villager, KillMethod::LoverDied), None);
        assert_eq!(death_locale_key(Role::Villager, KillMethod::HunterShot), None);
    }

    #[test]
    fn known_win_outcomes_map_to_their_real_keys() {
        assert_eq!(
            game_over_locale_key(&WinOutcome::Team(Team::Village)),
            Some("VillageWins")
        );
        assert_eq!(game_over_locale_key(&WinOutcome::Team(Team::Wolf)), Some("WolfWins"));
        assert_eq!(game_over_locale_key(&WinOutcome::Team(Team::Lovers)), Some("LoversWin"));
    }

    #[test]
    fn rng_dependent_outcomes_have_no_key_and_no_one_is_no_winner() {
        assert_eq!(game_over_locale_key(&WinOutcome::Team(Team::NoOne)), Some("NoWinner"));
        assert_eq!(game_over_locale_key(&WinOutcome::RngDependent("test")), None);
    }
}
