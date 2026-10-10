//! One file per role. Each declares the role's facts as `pub const DEF:
//! RoleDef` (see `definition`) and implements `RoleBehavior`, which turns the
//! player's answers into actions. `def` below maps every `Role` to its
//! `DEF` with an exhaustive match, so a new `Role` variant does not build
//! until it has a file and a line here.
//!
//! What a role *does* once its actions are in is resolved by the night
//! (`night::resolve_night_sequence`, region by region in the legacy order)
//! and the day (`orchestrator::apply_day_results`); each role's file says
//! where. `rust/ARCHITECTURE.md` walks through adding, changing and
//! deleting a role.

pub mod alpha_wolf;
pub mod apprentice_seer;
pub mod arsonist;
pub mod augur;
pub mod barkeep;
pub mod beholder;
pub mod blacksmith;
pub mod chef;
pub mod chemist;
pub mod clumsy_guy;
pub mod cultist;
pub mod cultist_hunter;
pub mod cupid;
pub mod cursed;
pub mod definition;
pub mod detective;
pub mod doppelganger;
pub mod drunk;
pub mod fool;
pub mod grave_digger;
pub mod guardian_angel;
pub mod gunner;
pub mod harlot;
pub mod hunter;
pub mod lovers;
pub mod lycan;
pub mod mason;
pub mod mayor;
pub mod oracle;
pub mod pacifist;
pub mod prince;
pub mod sandman;
pub mod seer;
pub mod serial_killer;
pub mod snow_wolf;
pub mod sorcerer;
pub mod spumpkin;
pub mod tanner;
pub mod thief;
pub mod traitor;
pub mod troublemaker;
pub mod villager;
pub mod wild_child;
pub mod wise_elder;
pub mod witch;
pub mod wolf;
pub mod wolf_cub;
pub mod wolf_man;

pub use definition::RoleDef;
pub use lovers::LoversState;

use shared::Role;

/// Deliberately just a `u64` wrapper, not a struct with role/alive-state
/// attached — a role's logic shouldn't be able to reach into another
/// player's private state, only refer to them by id and act through the
/// actions it returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerId(pub u64);

/// What a player chose at night. Roles only describe intent; the night
/// (`night::resolve_night_sequence`) applies these, e.g. tallying the
/// `EatVote`s and attacking whoever the wolves agree on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NightAction {
    EatVote {
        target: PlayerId,
    },
    CheckTeam {
        target: PlayerId,
    },
    Heal {
        target: PlayerId,
    },
    Poison {
        target: PlayerId,
    },
    LinkLovers {
        a: PlayerId,
        b: PlayerId,
    },
    /// Harlot visiting a target overnight (Werewolf.cs:2358-2360,
    /// 2424-2428). Carries `visitor` (not just `target`) so
    /// the Harlot region of `night::resolve_night_sequence` can kill the right
    /// player: visiting whoever the wolves' actually ate that same night
    /// kills the Harlot (Werewolf.cs:2352-2360, `KillMthd.VisitWolf`,
    /// unconditional — no RNG involved). Every other visit outcome is
    /// `night::Night::visit`.
    Visit {
        visitor: PlayerId,
        target: PlayerId,
    },
    /// Guardian Angel protecting a target overnight (Werewolf.cs:2361-2372,
    /// 3091 onward). Same caveat as `Visit`: the actual protection
    /// resolution (does it save them, does the GA risk dying) isn't
    /// modeled here yet.
    Protect {
        target: PlayerId,
    },
    /// Fool, Sorcerer, Oracle or Cultist Hunter looking at a target.
    /// What each learns is resolved by `reveals::resolve_night_readings`
    /// (Cultist Hunter excepted). The Detective snoops by day instead, see
    /// `DayAction::Snoop`.
    Investigate {
        target: PlayerId,
    },
    /// Wild Child's (and Doppelganger's — same shape, different transform
    /// payoff) one-time pick of a "role model" player (Werewolf.cs:1757,
    /// 1909 for Wild Child; 1927-1938 for Doppelganger). If that player
    /// dies, `game::apply_transforms` turns the picker into a Wolf (Wild
    /// Child) or a copy of the dead player's role (Doppelganger).
    ChooseRoleModel {
        target: PlayerId,
    },
    /// The cult's proposed conversion target for tonight
    /// (Werewolf.cs:3690-3735 onward: each surviving cultist effectively
    /// votes by choosing a target, with a per-target-role success chance).
    /// The conversion itself is the cult region of the night.
    ConvertVote {
        target: PlayerId,
    },
    /// The Serial Killer's personal kill choice (Werewolf.cs:2340-2348,
    /// 2380 onward). Unlike `EatVote`, there's normally only one Serial
    /// Killer, so there's no consensus to tally.
    SerialKillVote {
        target: PlayerId,
    },
    /// Thief stealing a target's role, night 1 only (Werewolf.cs:4132-4163:
    /// `StealRole(thief, target)`). Unlike Doppelganger's `ChooseRoleModel`
    /// (which waits for the role model to die), the steal is immediate —
    /// modeled as its own variant so a reader isn't left wondering why an
    /// "immediate" and a "wait for death" mechanic share one action shape.
    /// Carries `thief` (its own actor) so the Thief region of the night can
    /// apply the swap: the Thief becomes the target's role, and the target
    /// becomes a Villager (Werewolf.cs:2483-2486). In `ThiefFull` mode the
    /// steal can fail (Werewolf.cs:4173).
    StealRole {
        thief: PlayerId,
        target: PlayerId,
    },
    /// Chemist's nightly gambit (Werewolf.cs:3792-3821): visit a target,
    /// with an RNG chance of killing them (`KillMthd.Chemistry`) or, on
    /// failure, killing the Chemist instead — see
    /// the Chemist region of `night::resolve_night_sequence`, which needs `chemist`'s own id
    /// for exactly that self-kill case, the same reasoning `Shoot` and
    /// `StealRole` already carry their actor's id for.
    Chemistry {
        chemist: PlayerId,
        target: PlayerId,
    },
    /// Arsonist dousing a target with kerosene, any night, as many nights
    /// as they like (Werewolf.cs:3792 region / `x.Doused`) — there's no
    /// `primary_used`-style gate in the legacy code, doused players just
    /// accumulate in `night::NightMemory::doused`.
    Douse {
        target: PlayerId,
    },
    /// Arsonist choosing to detonate ("Spark", Werewolf.cs:1013,
    /// `player.Choice = -2`) instead of dousing this night: burns every
    /// doused player at once (`KillMthd.Burn`, Werewolf.cs:3198-3202) — see
    /// the Arsonist region of `night::resolve_night_sequence`, which `game::run_game`
    /// calls with this action and the cross-round doused set it keeps.
    Detonate,
    /// The Snow Wolf going out to freeze a target (Werewolf.cs:3092-3184).
    /// The freeze itself is resolved by `freeze::apply_freeze`; this
    /// records that the Snow Wolf left home, which a cult visit checks
    /// (Werewolf.cs:3688).
    Freeze {
        snow_wolf: PlayerId,
        target: PlayerId,
    },
}

/// Everything a role's `night_action` needs to decide what to do.
/// `chosen_*` fields represent a player's already-collected input (from
/// Telegram, in the real bot) — a role's job is to validate and shape that
/// input into a `NightAction`, not to decide who the player picks.
pub struct NightContext<'a> {
    pub alive: &'a [PlayerId],
    pub self_id: PlayerId,
    pub chosen_target: Option<PlayerId>,
    pub heal_target: Option<PlayerId>,
    pub poison_target: Option<PlayerId>,
    pub love_targets: Option<(PlayerId, PlayerId)>,
    /// The wolves' resolved target for tonight, if it's already known by
    /// the time this role is being asked: `None` when the wolves have not
    /// voted yet or chose nobody.
    pub wolf_target: Option<PlayerId>,
    /// Generic yes/no decision, for the roles whose action isn't "pick a
    /// player" at all — e.g. Sandman's once-per-game "put everyone to
    /// sleep?" (Werewolf.cs:950-961). Same "generic slot, documented per
    /// role" idea as `RoleState`'s fields; kept separate from
    /// `chosen_target` rather than repurposing `Some`/`None` as a boolean,
    /// since that would make a reader guess whether presence or the
    /// specific value mattered.
    pub toggle_choice: bool,
}

/// What a day action resolves to. Separate from `NightAction` because it's
/// a genuinely different point in the game loop (resolved during the day
/// phase, alongside lynch voting, not during night resolution) — conflating
/// the two just because they're both "an action a role takes" would hide
/// that distinction from a reader, not clarify it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayAction {
    /// Gunner spending a bullet to shoot a suspected player
    /// (Werewolf.cs:2871-2896). Carries `shooter` (not just `target`) so
    /// `game::run_game` can apply the Wise Elder special case
    /// (Werewolf.cs:2887-2889: shooting a Wise Elder demotes the *Gunner*
    /// to Villager, on top of the Wise Elder still dying normally) without
    /// needing to guess who fired — every other role's day/night action
    /// already gets its own actor for free via `ctx.self_id`; `Shoot` just
    /// didn't carry it forward into the action itself until this needed it.
    Shoot { shooter: PlayerId, target: PlayerId },
    /// Blacksmith spreading protective silver dust over the whole
    /// village, once (Werewolf.cs:935, 5083-5092: a yes/no menu, not a
    /// target pick). The effect lands the *following*
    /// night: every wolf-team role gets no valid targets at all
    /// (Werewolf.cs:5191: `if (!_silverSpread) { ...assign wolf targets...
    /// }` — nothing runs in the `else`), so `game::run_game_with_rules`
    /// skips the wolves that night.
    SpreadSilver,
    /// Mayor publicly revealing their role, once
    /// (Werewolf.cs:899-908). Purely informational on its own — the real
    /// payoff (their lynch vote counting twice afterward,
    /// Werewolf.cs:2649-2652) is handled by `orchestrator::resolve_day`
    /// directly, since it's a change to vote *tallying*, not a death.
    Reveal,
    /// Pacifist vetoing the day's lynch entirely, once
    /// (Werewolf.cs:913-925: `_pacifistUsed = true`). Applied by
    /// `apply_day_results`, which cancels the resolved lynch target if
    /// this action is present.
    Pacify,
    /// Troublemaker forcing a second lynch vote the same day, once
    /// (Werewolf.cs:965-973: `_doubleLynch = true`), and explicitly
    /// overriding a Pacifist's veto if both fire the same day
    /// (Werewolf.cs:971: `_pacifistUsed = false; // trouble overrides peace`)
    /// — whichever of the two acted last wins (`orchestrator::
    /// last_day_veto`). `game::run_game` holds the second lynch vote.
    Trouble,
    /// Spumpkin's day detonation (Werewolf.cs:2901-2926): pick a target,
    /// with an RNG chance (40%, Werewolf.cs:2907) of killing both the
    /// target and the Spumpkin himself — see
    /// `orchestrator::resolve_spumpkin_detonation`, which needs
    /// `spumpkin`'s own id for the mutual-kill, same reasoning `Shoot`
    /// carries `shooter`.
    Detonate { spumpkin: PlayerId, target: PlayerId },
    /// Detective snooping on a target by day (Werewolf.cs:2933-2953). What
    /// they learn is sent by `reveals::resolve_detective`.
    Snoop { detective: PlayerId, target: PlayerId },
    /// Sandman putting everyone to sleep, once: the next night is skipped
    /// (Werewolf.cs:950-961, 3011-3020).
    Sleep,
}

/// Same shape as `NightContext`, for the day phase. Kept as its own type
/// rather than reusing `NightContext` even though today it would only need
/// one extra field — day-phase context (e.g. who's currently nominated for
/// lynch) will likely diverge further as more day-phase roles get ported,
/// and conflating the two contexts would make a reader wonder which fields
/// are actually meaningful for which phase.
pub struct DayContext<'a> {
    pub alive: &'a [PlayerId],
    pub self_id: PlayerId,
    pub chosen_target: Option<PlayerId>,
    /// See `NightContext::toggle_choice` — same idea, for day-phase
    /// yes/no decisions (Mayor's reveal, Pacifist's peace).
    pub toggle_choice: bool,
}

/// Per-player state that must persist across nights (e.g. "has the Witch
/// used her heal potion yet"). Kept generic — `primary_used`/
/// `secondary_used` rather than `heal_used`/`poison_used` — so a new
/// one-shot-ability role doesn't need a struct change, just a doc comment
/// explaining what its flag means. Same idea for `remembered_player`: e.g.
/// Wild Child's chosen role model, checked each round by
/// `game::apply_transforms` to see if that player has died.
#[derive(Debug, Clone, Copy, Default)]
pub struct RoleState {
    pub primary_used: bool,
    pub secondary_used: bool,
    pub remembered_player: Option<PlayerId>,
}

/// How a role turns its player's answers into actions. Most roles do
/// nothing (see `villager::Villager`), which is the default.
pub trait RoleBehavior: Sync {
    /// Called once per night. Returns zero, one, or (for roles with two
    /// resources, like the Witch) two actions.
    fn night_action(&self, _ctx: &NightContext, _state: &mut RoleState) -> Vec<NightAction> {
        vec![]
    }

    /// Called once per day (the Gunner's shot, the Mayor's reveal).
    fn day_action(&self, _ctx: &DayContext, _state: &mut RoleState) -> Vec<DayAction> {
        vec![]
    }
}

/// Every role's `DEF`. Exhaustive on purpose: no `_` arm, so adding a
/// `Role` variant without a role file is a compile error.
pub fn def(role: Role) -> &'static RoleDef {
    use Role::*;
    match role {
        Villager => &villager::DEF,
        Drunk => &drunk::DEF,
        Harlot => &harlot::DEF,
        Seer => &seer::DEF,
        Traitor => &traitor::DEF,
        GuardianAngel => &guardian_angel::DEF,
        Detective => &detective::DEF,
        Wolf => &wolf::DEF,
        Cursed => &cursed::DEF,
        Gunner => &gunner::DEF,
        Tanner => &tanner::DEF,
        Fool => &fool::DEF,
        WildChild => &wild_child::DEF,
        Beholder => &beholder::DEF,
        ApprenticeSeer => &apprentice_seer::DEF,
        Cultist => &cultist::DEF,
        CultistHunter => &cultist_hunter::DEF,
        Mason => &mason::DEF,
        Doppelganger => &doppelganger::DEF,
        Cupid => &cupid::DEF,
        Hunter => &hunter::DEF,
        SerialKiller => &serial_killer::DEF,
        Sorcerer => &sorcerer::DEF,
        AlphaWolf => &alpha_wolf::DEF,
        WolfCub => &wolf_cub::DEF,
        Blacksmith => &blacksmith::DEF,
        ClumsyGuy => &clumsy_guy::DEF,
        Mayor => &mayor::DEF,
        Prince => &prince::DEF,
        Lycan => &lycan::DEF,
        Pacifist => &pacifist::DEF,
        WiseElder => &wise_elder::DEF,
        Oracle => &oracle::DEF,
        Sandman => &sandman::DEF,
        WolfMan => &wolf_man::DEF,
        Thief => &thief::DEF,
        Troublemaker => &troublemaker::DEF,
        Chemist => &chemist::DEF,
        SnowWolf => &snow_wolf::DEF,
        GraveDigger => &grave_digger::DEF,
        Augur => &augur::DEF,
        Arsonist => &arsonist::DEF,
        Spumpkin => &spumpkin::DEF,
        Chef => &chef::DEF,
        Barkeep => &barkeep::DEF,
        Witch => &witch::DEF,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::ALL_ROLES;

    #[test]
    fn every_role_maps_to_its_own_definition() {
        for &role in ALL_ROLES {
            assert_eq!(def(role).role, role, "def({role:?}) returns another role's DEF");
        }
    }
}
