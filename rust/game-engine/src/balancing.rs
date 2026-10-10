//! Role assignment for a starting game — `Shared/GameBalancing.cs`
//! (`Balance`, `GetRoleList`, `GetStrength`), the piece `control::lobby`'s
//! own module doc flags as the reason `startgame`/`startchaos` are only
//! partially covered: a `Lobby` gets you a validated *roster of players*,
//! this module is what decides *which role* each of them gets before
//! `game::run_game` can even start.
//!
//! Three RNG-driven decisions, all uniform picks (`RngSource::pick_index`
//! — nothing here is a percent-chance roll, unlike most of `orchestrator`'s
//! RNG use): which wolf variant(s) to include, a Fisher-Yates shuffle of
//! the candidate role pool, and picking a replacement role for a couple of
//! the "fixup" substitutions below. `Shuffle<T>`'s actual algorithm
//! (Werewolf.cs:12-29 — rejection-sampled bytes from
//! `RNGCryptoServiceProvider`) is standard Fisher-Yates underneath the
//! cryptographic sampling; [`shuffle`] ports the algorithm, not the CSPRNG
//! particulars, the same way `RngSource::chance` ports `R.Next(100) <
//! chance` rather than .NET's specific PRNG.
//!
//! `disabled_roles` is this port's stand-in for the source's `IRole`
//! bitflag encoding (`roleflags.GetUniqueRoles()`) — a plain `&[Role]`
//! rather than porting the bitflag mechanism itself, since a flags-in-an-
//! `i64` encoding is a legacy storage detail, not a rule worth preserving
//! bit-for-bit.

use crate::rng::RngSource;
use shared::{Role, ALL_ROLES};

/// `GameBalancing.cs:32`: only the four base wolf roles — `SnowWolf` is
/// deliberately excluded here too (matches every other `WolfRoles`
/// reference already ported into `game-engine`, e.g. `game
/// ::apply_pending_alpha_bites`'s `is_wolf_muscle`, which is the *other*
/// wolf-team list that *does* include `SnowWolf` — the two lists mean
/// different things and this module needs the narrower one).
const WOLF_ROLES: [Role; 4] = [Role::Wolf, Role::AlphaWolf, Role::WolfCub, Role::Lycan];

/// `GameBalancing.cs:43`: roles whose team isn't "village" for balancing
/// purposes — used to compute team sizes and strength totals, not the
/// same partition as `shared::Team` (Thief and Sorcerer are their own
/// `Team` variants but count as "non-village" here for balance purposes
/// only, matching the source exactly).
const NON_VILLAGE_ROLES: [Role; 11] = [
    Role::Cultist,
    Role::SerialKiller,
    Role::Tanner,
    Role::Wolf,
    Role::AlphaWolf,
    Role::Sorcerer,
    Role::WolfCub,
    Role::Lycan,
    Role::Thief,
    Role::SnowWolf,
    Role::Arsonist,
];

/// `GameBalancing.cs:44`: roles whose presence is obvious/self-revealing
/// to the village, capped at one per three players to avoid an
/// unbalanced "everyone already knows who to trust" game.
const REVEALED_VILLAGE_ROLES: [Role; 6] = [
    Role::Blacksmith,
    Role::Mayor,
    Role::Pacifist,
    Role::Gunner,
    Role::Sandman,
    Role::Troublemaker,
];

fn is_non_village(role: Role) -> bool {
    NON_VILLAGE_ROLES.contains(&role)
}

/// Fisher-Yates (Durstenfeld), the same algorithm `Shuffle<T>`
/// (Werewolf.cs:12-29) implements over rejection-sampled CSPRNG bytes —
/// ported as the algorithm, via `RngSource::pick_index`, not the specific
/// byte-rejection sampling technique (see module doc).
fn shuffle(rng: &mut dyn RngSource, list: &mut [Role]) {
    let mut n = list.len();
    while n > 1 {
        let k = rng.pick_index(n);
        n -= 1;
        list.swap(k, n);
    }
}

/// `GameBalancing.cs::GetRoleList` (152-214): the candidate pool `balance`
/// shuffles and truncates to `player_count`. Picks 1-5 wolves first (one
/// per 5 players, `SnowWolf` included in *this* pick even though it's
/// excluded from [`WOLF_ROLES`] — matches the source's separate,
/// five-member `possiblewolves` list at 157-159), then adds one of every
/// other role `ALL_ROLES` has a legacy precedent for.
///
/// `Chef`/`Barkeep`/`Witch` are excluded outright — new roles this port
/// added that postdate `RoleConfigHelper.GetRoles()` entirely, with no
/// legacy strength value or balancing precedent to assign them by (see
/// [`role_strength`]'s own note on the same three roles).
fn candidate_role_pool(rng: &mut dyn RngSource, player_count: usize, disabled_roles: &[Role]) -> Vec<Role> {
    let mut roles_to_assign = vec![];

    let mut possible_wolves: Vec<Role> = [Role::Wolf, Role::AlphaWolf, Role::WolfCub, Role::Lycan, Role::SnowWolf]
        .into_iter()
        .filter(|r| !disabled_roles.contains(r))
        .collect();

    if !possible_wolves.is_empty() {
        let wolf_count = (player_count / 5).clamp(1, 5);
        let mut wolf_to_add = possible_wolves[rng.pick_index(possible_wolves.len())];
        for _ in 0..wolf_count {
            roles_to_assign.push(wolf_to_add);
            if wolf_to_add != Role::Wolf {
                possible_wolves.retain(|&r| r != wolf_to_add);
            }
            if possible_wolves.is_empty() {
                break;
            }
            wolf_to_add = possible_wolves[rng.pick_index(possible_wolves.len())];
        }
    }

    // Werewolf.cs:170-171: a lone Snow Wolf (nobody to eat with) is
    // pointless - promoted to a plain Wolf instead.
    if roles_to_assign.len() == 1 && roles_to_assign[0] == Role::SnowWolf {
        roles_to_assign[0] = Role::Wolf;
    }

    for &role in ALL_ROLES {
        match role {
            Role::Wolf | Role::Lycan | Role::WolfCub | Role::AlphaWolf | Role::SnowWolf => {}
            Role::CultistHunter | Role::Cultist => {
                if player_count > 10 {
                    roles_to_assign.push(role);
                }
            }
            Role::Spumpkin => {}
            Role::Chef | Role::Barkeep | Role::Witch => {}
            _ => roles_to_assign.push(role),
        }
    }

    roles_to_assign.push(Role::Mason);
    roles_to_assign.push(Role::Mason);

    if roles_to_assign.contains(&Role::CultistHunter) {
        roles_to_assign.push(Role::Cultist);
        roles_to_assign.push(Role::Cultist);
    }

    for _ in 0..(player_count / 4) {
        roles_to_assign.push(Role::Villager);
    }

    roles_to_assign.retain(|r| !disabled_roles.contains(r));
    roles_to_assign
}

/// `GameBalancing.cs::GetStrength` (216-314): a rough "how much does this
/// role swing the game" score, some context-dependent on the rest of
/// `all_roles` (a `Seer` is worth less if a `Lycan`/`WolfMan` can spoof
/// her check, a `Cursed` is worth less the more wolves could bite them,
/// ...). Used only to keep non-chaos games within `balance`'s variance
/// threshold - `chaos` mode skips this scoring entirely, matching the
/// source (`if (!chaos) { ...strength checks... }`).
///
/// Exhaustive over every `Role`, including `Chef`/`Barkeep`/`Witch` (which
/// [`candidate_role_pool`] never actually includes) - `0` for all three,
/// same treatment `Thief`/`Chemist` already get for "doesn't meaningfully
/// swing the balance," not a real legacy value (there isn't one; these
/// roles postdate `GameBalancing.cs`).
pub fn role_strength(role: Role, all_roles: &[Role]) -> i32 {
    let wolf_roles = WOLF_ROLES;
    let non_convertible_roles: [Role; 13] = [
        Role::Seer,
        Role::GuardianAngel,
        Role::Detective,
        Role::Cursed,
        Role::Harlot,
        Role::Hunter,
        Role::Doppelganger,
        Role::Wolf,
        Role::AlphaWolf,
        Role::WolfCub,
        Role::SerialKiller,
        Role::Lycan,
        Role::Thief,
    ];
    let count = |r: Role| all_roles.iter().filter(|&&x| x == r).count() as i32;

    match role {
        Role::Villager => 1,
        Role::Drunk => 3,
        Role::Harlot => 6,
        Role::Seer => 7 - count(Role::Lycan) - (count(Role::WolfMan) * 2),
        Role::Traitor => 0,
        Role::GuardianAngel => 7 + i32::from(all_roles.contains(&Role::Arsonist)),
        Role::Detective => 6,
        Role::Wolf => 10,
        Role::Cursed => {
            1 - all_roles
                .iter()
                .filter(|&&x| wolf_roles.contains(&x) || x == Role::SnowWolf)
                .count() as i32
                / 2
        }
        Role::Gunner => 6,
        Role::Tanner => all_roles.len() as i32 / 2,
        Role::Fool => 3,
        Role::WildChild => 1,
        Role::Beholder => {
            1 + i32::from(all_roles.contains(&Role::Seer)) * 4 + i32::from(all_roles.contains(&Role::Fool))
        }
        Role::ApprenticeSeer => 6,
        Role::Cultist => {
            10 + all_roles
                .iter()
                .filter(|&&x| !non_convertible_roles.contains(&x))
                .count() as i32
        }
        Role::CultistHunter => {
            if count(Role::Cultist) == 0 {
                1
            } else {
                7
            }
        }
        Role::Mason => {
            let masons = count(Role::Mason);
            if masons <= 1 {
                1
            } else {
                masons + 3
            }
        }
        Role::Doppelganger => 2,
        Role::Cupid => 2,
        Role::Hunter => 6,
        Role::SerialKiller => 15,
        Role::Sorcerer => 2,
        Role::AlphaWolf => 12,
        Role::WolfCub => {
            let can_have_another_wolf = [
                Role::AlphaWolf,
                Role::Wolf,
                Role::Lycan,
                Role::SnowWolf,
                Role::WildChild,
                Role::Doppelganger,
                Role::Cursed,
                Role::Traitor,
            ]
            .iter()
            .any(|r| all_roles.contains(r));
            if can_have_another_wolf {
                12
            } else {
                10
            }
        }
        Role::Blacksmith => 5,
        Role::ClumsyGuy => -1,
        Role::Mayor => 4,
        Role::Prince => 3,
        Role::WolfMan => 1,
        Role::Augur => 5,
        Role::Pacifist => 3,
        Role::WiseElder => 3,
        Role::Oracle => 4,
        Role::Sandman => 3,
        Role::Lycan => 10,
        // Werewolf.cs:296-298: "Testing 0 instead of 4 since thief
        // doesn't really seem to 'harm' the village team a lot" - the
        // source's own comment, kept verbatim since it explains a
        // deliberately-tuned value that looks like it could be a typo.
        Role::Thief => 0,
        Role::Troublemaker => 5,
        Role::Chemist => 0,
        Role::SnowWolf => 15,
        Role::GraveDigger => 5,
        Role::Arsonist => 8,
        Role::Spumpkin => 2,
        Role::Chef | Role::Barkeep | Role::Witch => 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BalanceError {
    /// `GameBalancing.cs:50-53`: 500 attempts without landing on a
    /// balanced roster - the source throws; this port reports instead,
    /// same "let the caller decide what 'couldn't balance' means" choice
    /// every other fallible boundary in this crate makes.
    UnableToBalance { player_count: usize },
}

/// `GameBalancing.cs::Balance` (34-133): repeatedly build a candidate
/// pool, shuffle it down to `player_count`, patch a few structurally-bad
/// results (a `Sorcerer`/`Traitor`/`SnowWolf` with no wolves to serve,
/// a `Cultist` with no `CultistHunter` to oppose them, an
/// `ApprenticeSeer` with no `Seer` to eventually replace), then check
/// the result is actually a game (two real teams, baddies outnumbered,
/// and - only outside `chaos` mode - within a strength-variance budget)
/// before accepting it. Retries up to 500 times.
pub fn balance(
    player_count: usize,
    chaos: bool,
    disabled_roles: &[Role],
    burning_overkill: bool,
    rng: &mut dyn RngSource,
) -> Result<Vec<Role>, BalanceError> {
    balance_with_pool(player_count, chaos, disabled_roles, burning_overkill, rng).map(|(roles, _)| roles)
}

/// `balance`, also returning the candidate pool the roles were dealt from
/// (GameBalancing.cs:60: `possibleRoles = new List<IRole>(rolesToAssign)`,
/// taken before the shuffle-and-truncate). The Augur draws the roles
/// "not in the village" from this pool (Werewolf.cs:4049-4052), so pass it
/// to `game::run_game_with_role_pool`.
pub fn balance_with_pool(
    player_count: usize,
    chaos: bool,
    disabled_roles: &[Role],
    burning_overkill: bool,
    rng: &mut dyn RngSource,
) -> Result<(Vec<Role>, Vec<Role>), BalanceError> {
    for _attempt in 0..500 {
        let mut roles_to_assign = candidate_role_pool(rng, player_count, disabled_roles);
        while roles_to_assign.len() < player_count {
            roles_to_assign.push(Role::Villager);
        }
        let pool = roles_to_assign.clone();
        shuffle(rng, &mut roles_to_assign);
        roles_to_assign.truncate(player_count);

        // Werewolf.cs:66-73: pointless without a wolf to serve - convert
        // one to a (disabled-role-aware) random wolf variant.
        if (roles_to_assign.contains(&Role::Sorcerer)
            || roles_to_assign.contains(&Role::Traitor)
            || roles_to_assign.contains(&Role::SnowWolf))
            && !roles_to_assign.iter().any(|r| WOLF_ROLES.contains(r))
        {
            if let Some(idx) = roles_to_assign
                .iter()
                .position(|&r| r == Role::Sorcerer || r == Role::Traitor || r == Role::SnowWolf)
            {
                let possible_wolves: Vec<Role> = WOLF_ROLES
                    .iter()
                    .copied()
                    .filter(|r| !disabled_roles.contains(r))
                    .collect();
                // Guards against an empty `possible_wolves` (every base
                // wolf role disabled) - the source has no such guard and
                // would throw indexing an empty list; skipping the
                // substitution here is a safe divergence from a crash,
                // not a behavior this port is trying to preserve.
                if !possible_wolves.is_empty() {
                    roles_to_assign[idx] = possible_wolves[rng.pick_index(possible_wolves.len())];
                }
            }
        }

        // Werewolf.cs:75-82: a cult with nobody hunting them.
        if roles_to_assign.contains(&Role::Cultist)
            && !roles_to_assign.contains(&Role::CultistHunter)
            && !disabled_roles.contains(&Role::CultistHunter)
        {
            if let Some(idx) = roles_to_assign.iter().position(|r| !is_non_village(*r)) {
                roles_to_assign[idx] = Role::CultistHunter;
            }
        }

        // Werewolf.cs:84-90: an Apprentice Seer with no Seer to promote
        // from when the Seer dies - just make them the Seer outright.
        if roles_to_assign.contains(&Role::ApprenticeSeer) && !roles_to_assign.contains(&Role::Seer) {
            if let Some(idx) = roles_to_assign.iter().position(|&r| r == Role::ApprenticeSeer) {
                roles_to_assign[idx] = Role::Seer;
            }
        }

        let mut balanced = roles_to_assign.iter().any(|r| !is_non_village(*r))
            && roles_to_assign
                .iter()
                .any(|&r| is_non_village(r) && r != Role::Sorcerer && r != Role::Tanner && r != Role::Thief);

        if roles_to_assign.iter().filter(|r| is_non_village(**r)).count()
            >= roles_to_assign.iter().filter(|r| !is_non_village(**r)).count()
        {
            balanced = false;
        }

        if roles_to_assign.contains(&Role::SerialKiller) && roles_to_assign.contains(&Role::Arsonist) {
            balanced = balanced && burning_overkill;
        }

        if !chaos {
            let village_strength: i32 = roles_to_assign
                .iter()
                .filter(|r| !is_non_village(**r))
                .map(|&r| role_strength(r, &roles_to_assign))
                .sum();
            let enemy_strength: i32 = roles_to_assign
                .iter()
                .filter(|r| is_non_village(**r))
                .map(|&r| role_strength(r, &roles_to_assign))
                .sum();
            let variance_allowed = (player_count / 4) as i32 + 1;
            balanced = balanced && (village_strength - enemy_strength).abs() <= variance_allowed;

            let revealed_count = roles_to_assign
                .iter()
                .filter(|r| REVEALED_VILLAGE_ROLES.contains(r))
                .count();
            if revealed_count * 3 > roles_to_assign.len() {
                balanced = false;
            }

            let only_wolf_baddies = !roles_to_assign
                .iter()
                .any(|r| matches!(r, Role::Arsonist | Role::SerialKiller | Role::Cultist));
            let kill_stopping_count = roles_to_assign
                .iter()
                .filter(|&&r| {
                    r == Role::Troublemaker || r == Role::Sandman || (only_wolf_baddies && r == Role::Blacksmith)
                })
                .count();
            if kill_stopping_count * 4 > roles_to_assign.len() {
                balanced = false;
            }
        }

        if balanced {
            return Ok((roles_to_assign, pool));
        }
    }

    Err(BalanceError::UnableToBalance { player_count })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::SeededRngSource;

    #[test]
    fn balances_a_typical_five_player_game_for_many_seeds() {
        for seed in 0..200 {
            let mut rng = SeededRngSource::new(seed);
            let roles = balance(5, false, &[], true, &mut rng)
                .unwrap_or_else(|e| panic!("seed {seed} failed to balance: {e:?}"));
            assert_eq!(roles.len(), 5);
        }
    }

    #[test]
    fn balances_chaos_mode_too() {
        for seed in 0..200 {
            let mut rng = SeededRngSource::new(seed);
            let roles = balance(8, true, &[], true, &mut rng)
                .unwrap_or_else(|e| panic!("seed {seed} failed to balance: {e:?}"));
            assert_eq!(roles.len(), 8);
        }
    }

    #[test]
    fn balances_a_large_game_including_the_cult() {
        let mut rng = SeededRngSource::new(1);
        let roles = balance(20, false, &[], true, &mut rng).expect("20p game should balance");
        assert_eq!(roles.len(), 20);
    }

    #[test]
    fn a_sorcerer_traitor_or_snow_wolf_never_ends_up_with_no_wolves() {
        for seed in 0..300 {
            let mut rng = SeededRngSource::new(seed);
            let roles = balance(10, false, &[], true, &mut rng).unwrap();
            let has_wolf = roles.iter().any(|r| WOLF_ROLES.contains(r));
            let needs_wolf =
                roles.contains(&Role::Sorcerer) || roles.contains(&Role::Traitor) || roles.contains(&Role::SnowWolf);
            assert!(
                !needs_wolf || has_wolf,
                "seed {seed}: {roles:?} has a wolf-dependent role but no wolf"
            );
        }
    }

    #[test]
    fn a_cultist_never_ends_up_without_a_cultist_hunter() {
        for seed in 0..300 {
            let mut rng = SeededRngSource::new(seed);
            let roles = balance(12, false, &[], true, &mut rng).unwrap();
            if roles.contains(&Role::Cultist) {
                assert!(roles.contains(&Role::CultistHunter), "seed {seed}: {roles:?}");
            }
        }
    }

    #[test]
    fn an_apprentice_seer_never_ends_up_without_a_seer() {
        for seed in 0..300 {
            let mut rng = SeededRngSource::new(seed);
            let roles = balance(10, false, &[], true, &mut rng).unwrap();
            if roles.contains(&Role::ApprenticeSeer) {
                assert!(roles.contains(&Role::Seer), "seed {seed}: {roles:?}");
            }
        }
    }

    #[test]
    fn baddies_never_outnumber_or_match_the_village() {
        for seed in 0..300 {
            let mut rng = SeededRngSource::new(seed);
            let roles = balance(9, false, &[], true, &mut rng).unwrap();
            let baddies = roles.iter().filter(|r| is_non_village(**r)).count();
            let village = roles.iter().filter(|r| !is_non_village(**r)).count();
            assert!(baddies < village, "seed {seed}: {roles:?}");
        }
    }

    #[test]
    fn disabled_roles_never_appear() {
        let disabled = [Role::SerialKiller, Role::Arsonist, Role::Cultist, Role::CultistHunter];
        for seed in 0..100 {
            let mut rng = SeededRngSource::new(seed);
            let roles = balance(10, false, &disabled, true, &mut rng).unwrap();
            for d in disabled {
                assert!(
                    !roles.contains(&d),
                    "seed {seed}: disabled role {d:?} appeared in {roles:?}"
                );
            }
        }
    }

    #[test]
    fn chef_barkeep_and_witch_never_appear_no_legacy_precedent() {
        for seed in 0..100 {
            let mut rng = SeededRngSource::new(seed);
            let roles = balance(15, true, &[], true, &mut rng).unwrap();
            assert!(!roles.contains(&Role::Chef));
            assert!(!roles.contains(&Role::Barkeep));
            assert!(!roles.contains(&Role::Witch));
        }
    }

    #[test]
    fn shuffle_is_a_permutation_not_a_resample() {
        let mut rng = SeededRngSource::new(42);
        let mut roles = vec![Role::Wolf, Role::Villager, Role::Seer, Role::Hunter, Role::Mason];
        let original = roles.clone();
        shuffle(&mut rng, &mut roles);
        assert_eq!(roles.len(), original.len());
        for r in &original {
            assert_eq!(roles.iter().filter(|x| *x == r).count(), 1);
        }
    }

    #[test]
    fn role_strength_matches_known_legacy_values_with_no_context() {
        let empty: Vec<Role> = vec![];
        assert_eq!(role_strength(Role::Villager, &empty), 1);
        assert_eq!(role_strength(Role::Wolf, &empty), 10);
        assert_eq!(role_strength(Role::SerialKiller, &empty), 15);
        assert_eq!(role_strength(Role::SnowWolf, &empty), 15);
        assert_eq!(role_strength(Role::ClumsyGuy, &empty), -1);
        assert_eq!(role_strength(Role::Thief, &empty), 0);
    }

    #[test]
    fn seer_strength_drops_with_lycan_and_wolf_man() {
        let empty: Vec<Role> = vec![];
        assert_eq!(role_strength(Role::Seer, &empty), 7);
        assert_eq!(role_strength(Role::Seer, &[Role::Lycan]), 6);
        assert_eq!(role_strength(Role::Seer, &[Role::WolfMan]), 5);
    }

    #[test]
    fn mason_strength_rewards_more_masons() {
        assert_eq!(role_strength(Role::Mason, &[Role::Mason]), 1);
        assert_eq!(role_strength(Role::Mason, &[Role::Mason, Role::Mason]), 5);
    }
}
