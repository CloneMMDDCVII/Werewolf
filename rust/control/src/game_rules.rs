//! Turns a group's stored settings into what one game plays by
//! (Werewolf.cs:174-211): the game mode, the engine's `GameRules`, and
//! burning overkill for balancing. Random mode rolls each of them fresh
//! for every game instead of reading the stored values.
//!
//! One stated divergence: in random mode the legacy node rolls
//! `ShowRolesOnDeath` and `ShowRolesEnd` but then reads the group's stored
//! flags when it reveals roles (Werewolf.cs:2765, 4906), so the rolls never
//! showed. Here the rolled values are the ones used.
//!
//! ShowIDs, ShufflePlayerList and AllowNSFW are display-only: the presenter
//! reads them from `GroupSettings` directly, since the engine decides
//! nothing for them.

use crate::group_config::{EndRoles as StoredEndRoles, GroupSettings, Toggle};
use game_engine::rng::RngSource;
use game_engine::rules::{EndRoles, GameRules, SecretLynch, Timers};
use shared::GameMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameStart {
    pub mode: GameMode,
    pub rules: GameRules,
    pub burning_overkill: bool,
}

fn end_roles(stored: StoredEndRoles) -> EndRoles {
    match stored {
        StoredEndRoles::None => EndRoles::None,
        StoredEndRoles::Living => EndRoles::Living,
        StoredEndRoles::All => EndRoles::All,
    }
}

fn secret_lynch(enabled: bool, show_votes: bool, show_voters: bool) -> Option<SecretLynch> {
    enabled.then_some(SecretLynch {
        show_votes,
        show_voters,
    })
}

/// What a game started as `requested_mode` in this group plays by.
pub fn game_start(settings: &GroupSettings, requested_mode: GameMode, rng: &mut dyn RngSource) -> GameStart {
    let timers = Timers {
        day_seconds: settings.day_seconds,
        night_seconds: settings.night_seconds,
        lynch_seconds: settings.lynch_seconds,
    };
    if settings.is_on(Toggle::RandomMode) {
        // Werewolf.cs:181-197, rolled in the same order. AllowArsonist is
        // rolled there too; it has no setting in this port (the Arsonist
        // is balanced like any other role), so it is not rolled here.
        let modes = [GameMode::Normal, GameMode::Chaos];
        let mode = modes[rng.pick_index(modes.len())];
        let thief_full = rng.chance(50);
        let secret = rng.chance(50);
        let show_roles_on_death = rng.chance(50);
        let show_votes = rng.chance(50);
        let show_voters = rng.chance(50);
        let burning_overkill = rng.chance(1);
        let end_roles = match rng.pick_index(100) {
            0..=32 => EndRoles::None,
            33..=66 => EndRoles::Living,
            _ => EndRoles::All,
        };
        return GameStart {
            mode,
            rules: GameRules {
                show_roles_on_death,
                end_roles,
                secret_lynch: secret_lynch(secret, show_votes, show_voters),
                thief_full,
                timers,
            },
            burning_overkill,
        };
    }
    GameStart {
        mode: settings.mode.resolve(requested_mode),
        rules: GameRules {
            show_roles_on_death: settings.is_on(Toggle::ShowRolesDeath),
            end_roles: end_roles(settings.end_roles),
            secret_lynch: secret_lynch(
                settings.is_on(Toggle::EnableSecretLynch),
                settings.is_on(Toggle::SecretLynchShowVotes),
                settings.is_on(Toggle::SecretLynchShowVoters),
            ),
            thief_full: settings.is_on(Toggle::ThiefFull),
            timers,
        },
        burning_overkill: settings.is_on(Toggle::BurningOverkill),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group_config::ModeSetting;
    use game_engine::rng::SeededRngSource;

    #[test]
    fn default_settings_give_the_default_rules() {
        let start = game_start(&GroupSettings::default(), GameMode::Chaos, &mut SeededRngSource::new(1));
        assert_eq!(start.rules, GameRules::default());
        assert_eq!(start.mode, GameMode::Chaos);
        assert!(!start.burning_overkill);
    }

    #[test]
    fn stored_flags_and_timers_reach_the_rules() {
        let mut settings = GroupSettings::default();
        settings.mode = ModeSetting::Normal;
        settings.set(Toggle::ShowRolesDeath, false);
        settings.set(Toggle::EnableSecretLynch, true);
        settings.set(Toggle::SecretLynchShowVoters, true);
        settings.set(Toggle::ThiefFull, true);
        settings.end_roles = StoredEndRoles::Living;
        settings.day_seconds = 30;
        settings.night_seconds = 45;
        settings.lynch_seconds = 60;
        let start = game_start(&settings, GameMode::Chaos, &mut SeededRngSource::new(1));
        assert_eq!(start.mode, GameMode::Normal);
        assert_eq!(
            start.rules,
            GameRules {
                show_roles_on_death: false,
                end_roles: EndRoles::Living,
                secret_lynch: Some(SecretLynch {
                    show_votes: settings.is_on(Toggle::SecretLynchShowVotes),
                    show_voters: true
                }),
                thief_full: true,
                timers: Timers {
                    day_seconds: 30,
                    night_seconds: 45,
                    lynch_seconds: 60
                },
            }
        );
    }

    #[test]
    fn random_mode_ignores_the_stored_flags_and_keeps_the_timers() {
        let mut settings = GroupSettings::default();
        settings.set(Toggle::RandomMode, true);
        settings.night_seconds = 45;
        let mut seen_secret = [false; 2];
        let mut seen_end = [false; 3];
        for seed in 0..200 {
            let start = game_start(&settings, GameMode::Normal, &mut SeededRngSource::new(seed));
            assert_eq!(start.rules.timers.night_seconds, 45);
            seen_secret[start.rules.secret_lynch.is_some() as usize] = true;
            seen_end[start.rules.end_roles as usize] = true;
        }
        assert_eq!(seen_secret, [true, true]);
        assert_eq!(seen_end, [true, true, true]);
    }
}
