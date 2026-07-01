//! Every role's logic lives in its own file here, implementing one shared
//! trait (`RoleBehavior`). The point isn't just organization — it's that
//! `behavior_for` below is an **exhaustive match over every `Role`
//! variant**, so the compiler refuses to build if a role is ever added
//! without a line here saying what to do with it (even if that line is
//! just "not implemented yet"). That replaces the honor-system comment in
//! the legacy code (`Shared/Roles.cs:9`): *"If you add a role, make sure
//! to add it into the SetTeam and GetStrength methods!"* — a instruction
//! only a person can forget, not the compiler.
//!
//! For a newcomer: start at `behavior_for`, pick a role you're curious
//! about, and go read its one small file. Nothing else in the game
//! depends on you understanding any other role first.

pub mod cupid;
pub mod lovers;
pub mod seer;
pub mod villager;
pub mod witch;
pub mod wolf;

pub use lovers::LoversState;

use shared::{Role, Team};

/// Deliberately just a `u64` wrapper, not a struct with role/alive-state
/// attached — a role's logic shouldn't be able to reach into another
/// player's private state, only refer to them by id and act through the
/// actions it returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerId(pub u64);

/// What a night action resolves to. The orchestrator (not shown in this
/// proof-of-concept — it would live in `control` or a future `engine`
/// runtime loop) is responsible for actually applying these: e.g.
/// collecting all `EatVote`s and killing whoever the wolves agree on.
/// Roles only ever describe *intent*, never mutate game state directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NightAction {
    EatVote { target: PlayerId },
    CheckTeam { target: PlayerId },
    Heal { target: PlayerId },
    Poison { target: PlayerId },
    LinkLovers { a: PlayerId, b: PlayerId },
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
}

/// Per-player state that must persist across nights (e.g. "has the Witch
/// used her heal potion yet"). Kept generic — `primary_used`/
/// `secondary_used` rather than `heal_used`/`poison_used` — so a new
/// one-shot-ability role doesn't need a struct change, just a doc comment
/// explaining what its flag means.
#[derive(Debug, Clone, Copy, Default)]
pub struct RoleState {
    pub primary_used: bool,
    pub secondary_used: bool,
}

/// The seam every role implements. Two hooks for now (this is a proof of
/// concept, not the full engine) — more will be added as more of the
/// legacy game loop gets ported, each one another exhaustive match to keep
/// honest.
pub trait RoleBehavior {
    fn team(&self) -> Team;

    /// Called once per night for this role. Most roles do nothing (see
    /// `villager::Villager` — the template for a no-op role); returns
    /// zero, one, or (for multi-resource roles like Witch) two actions.
    fn night_action(&self, _ctx: &NightContext, _state: &mut RoleState) -> Vec<NightAction> {
        vec![]
    }
}

/// A role this proof-of-concept hasn't ported yet. Team is still correct
/// (reuses `shared::Role::team`), it just has no behavior wired up. This
/// is the explicit "not yet done" marker `behavior_for` uses for every role
/// outside the five built out so far — deliberately not a silent
/// catch-all, see the match below.
pub struct Unimplemented(pub Role);

impl RoleBehavior for Unimplemented {
    fn team(&self) -> Team {
        self.0.team()
    }
}

/// Dispatches a `Role` to its behavior. Exhaustive on purpose: every
/// variant is named explicitly, either routed to a real implementation or
/// to `Unimplemented`. A `_ => ...` wildcard arm would compile just as
/// happily whether or not a new role was handled — the whole point is that
/// this does *not* have one, so adding a `Role` variant without touching
/// this match is a compile error, not a runtime surprise.
pub fn behavior_for(role: Role) -> Box<dyn RoleBehavior> {
    use Role::*;
    match role {
        Villager => Box::new(villager::Villager),
        Wolf => Box::new(wolf::Wolf),
        Seer => Box::new(seer::Seer),
        Witch => Box::new(witch::Witch),
        Cupid => Box::new(cupid::Cupid),

        // Not yet ported to the new structure. Listed explicitly (not
        // behind `_`) so the exhaustiveness guarantee above actually holds.
        Drunk | Harlot | Traitor | GuardianAngel | Detective | Cursed | Gunner | Tanner
        | Fool | WildChild | Beholder | ApprenticeSeer | Cultist | CultistHunter | Mason
        | Doppelganger | Hunter | SerialKiller | Sorcerer | AlphaWolf | WolfCub | Blacksmith
        | ClumsyGuy | Mayor | Prince | Lycan | Pacifist | WiseElder | Oracle | Sandman
        | WolfMan | Thief | Troublemaker | Chemist | SnowWolf | GraveDigger | Augur
        | Arsonist | Spumpkin | Chef | Barkeep => Box::new(Unimplemented(role)),
    }
}
