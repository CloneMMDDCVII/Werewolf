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

pub mod alpha_wolf;
pub mod apprentice_seer;
pub mod beholder;
pub mod blacksmith;
pub mod clumsy_guy;
pub mod cultist;
pub mod cultist_hunter;
pub mod cupid;
pub mod cursed;
pub mod detective;
pub mod doppelganger;
pub mod drunk;
pub mod fool;
pub mod gunner;
pub mod guardian_angel;
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
pub mod sorcerer;
pub mod tanner;
pub mod traitor;
pub mod villager;
pub mod wild_child;
pub mod wise_elder;
pub mod witch;
pub mod wolf;
pub mod wolf_cub;

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
    /// Harlot visiting a target overnight (Werewolf.cs:2358-2360,
    /// 2424-2428). What happens if the visited player turns out to be a
    /// wolf, or is otherwise unavailable, is resolution logic this
    /// proof-of-concept doesn't model yet — see `harlot` module doc.
    Visit { target: PlayerId },
    /// Guardian Angel protecting a target overnight (Werewolf.cs:2361-2372,
    /// 3091 onward). Same caveat as `Visit`: the actual protection
    /// resolution (does it save them, does the GA risk dying) isn't
    /// modeled here yet.
    Protect { target: PlayerId },
    /// Detective/Fool investigating a target (Werewolf.cs:2933-2953 for
    /// Detective, 3985-4000 for Fool). What information comes back —
    /// and, for Detective, the chance of tipping off the wolves
    /// (Werewolf.cs:2937) — is resolution logic, not modeled here.
    Investigate { target: PlayerId },
    /// Wild Child's (and Doppelganger's — same shape, different transform
    /// payoff) one-time pick of a "role model" player (Werewolf.cs:1757,
    /// 1909 for Wild Child; 1927-1938 for Doppelganger). If that player
    /// dies, `game::apply_transforms` turns the picker into a Wolf (Wild
    /// Child) or a copy of the dead player's role (Doppelganger).
    ChooseRoleModel { target: PlayerId },
    /// The cult's proposed conversion target for tonight
    /// (Werewolf.cs:3690-3735 onward: each surviving cultist effectively
    /// votes by choosing a target, with a per-target-role success chance).
    /// This models only the *proposal*; the conversion itself is
    /// RNG-and-role-dependent resolution logic not modeled here.
    ConvertVote { target: PlayerId },
    /// The Serial Killer's personal kill choice (Werewolf.cs:2340-2348,
    /// 2380 onward). Unlike `EatVote`, there's normally only one Serial
    /// Killer, so there's no consensus to tally — `apply_night_results`
    /// applies this as a direct, unconditional kill.
    SerialKillVote { target: PlayerId },
    /// Sandman choosing to put everyone to sleep tonight
    /// (Werewolf.cs:950-961: `_sandmanSleep = true`), a once-per-game
    /// no-target toggle rather than picking a player. What "asleep"
    /// actually suppresses (other roles' actions) is resolution logic
    /// this proof-of-concept doesn't attempt.
    SandmanSleep,
}

/// A piece of information about tonight that some role's decision depends
/// on *before it can even be asked* — not a game rule, a scheduling fact.
/// The canonical example is Witch: she can't sensibly be asked "heal or
/// not?" until the wolves' target is known, so her night action has to
/// resolve in a later step than the wolves' vote, not the same one. See
/// `RoleBehavior::requires`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NightFact {
    WolfTarget,
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
    /// the time this role is being asked. `None` either means "no wolf
    /// vote happened yet" or "the wolves had no valid target" — this
    /// proof-of-concept doesn't yet distinguish the two, since there's no
    /// orchestrator to produce either case for real.
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
    /// (Werewolf.cs:2871-2896). What the shot actually does — kill
    /// outright, special-cased outcomes like the Wise Elder revert
    /// (Werewolf.cs:2887-2889) — is resolution logic for a future
    /// orchestrator; this only covers "is this a valid target to shoot."
    Shoot { target: PlayerId },
    /// Blacksmith spreading protective silver to a target
    /// (Werewolf.cs:935, 5083). Whether it actually protects them from a
    /// wolf attack is resolution logic this proof-of-concept doesn't
    /// attempt, same caveat as `NightAction::Visit`/`Protect`.
    SpreadSilver { target: PlayerId },
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

/// The seam every role implements. Three hooks for now (this is a proof of
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

    /// Called once per day for this role (e.g. Gunner's shot). Most roles
    /// have no day action at all — that's the default here, not an
    /// omission, the same way most roles don't override `night_action`.
    fn day_action(&self, _ctx: &DayContext, _state: &mut RoleState) -> Vec<DayAction> {
        vec![]
    }

    /// Declares what this role's `night_action` needs resolved *before* it
    /// can be meaningfully asked. Empty for almost every role — Wolf, Seer,
    /// Harlot etc. can all be asked simultaneously with no ordering
    /// constraint between them. A future orchestrator would use this to
    /// compute night resolution order (a topological sort, not a hardcoded
    /// phase list), so adding a new dependent role is a one-line addition
    /// here rather than a manual re-sequencing exercise somewhere else.
    fn requires(&self) -> &'static [NightFact] {
        &[]
    }

    /// Whether this role can currently be dealt into a real game. Not the
    /// same question as "is it implemented" — `Witch` has real, tested
    /// logic (see `witch` module) but stays gated off (`false`) because
    /// nothing resolves the `WolfTarget` dependency `requires()` declares
    /// yet, and shipping her without that would just mean asking the heal
    /// question with stale/wrong information. Default `true`: most roles
    /// have no such prerequisite.
    fn is_available(&self) -> bool {
        true
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
        Drunk => Box::new(drunk::Drunk),
        Harlot => Box::new(harlot::Harlot),
        Traitor => Box::new(traitor::Traitor),
        GuardianAngel => Box::new(guardian_angel::GuardianAngel),
        Detective => Box::new(detective::Detective),
        Cursed => Box::new(cursed::Cursed),
        Gunner => Box::new(gunner::Gunner),
        Tanner => Box::new(tanner::Tanner),
        Fool => Box::new(fool::Fool),
        WildChild => Box::new(wild_child::WildChild),
        Beholder => Box::new(beholder::Beholder),
        ApprenticeSeer => Box::new(apprentice_seer::ApprenticeSeer),
        Cultist => Box::new(cultist::Cultist),
        CultistHunter => Box::new(cultist_hunter::CultistHunter),
        Mason => Box::new(mason::Mason),
        Doppelganger => Box::new(doppelganger::Doppelganger),
        Hunter => Box::new(hunter::Hunter),
        SerialKiller => Box::new(serial_killer::SerialKiller),
        Sorcerer => Box::new(sorcerer::Sorcerer),
        AlphaWolf => Box::new(alpha_wolf::AlphaWolf),
        WolfCub => Box::new(wolf_cub::WolfCub),
        Blacksmith => Box::new(blacksmith::Blacksmith),
        ClumsyGuy => Box::new(clumsy_guy::ClumsyGuy),
        Mayor => Box::new(mayor::Mayor),
        Prince => Box::new(prince::Prince),
        Lycan => Box::new(lycan::Lycan),
        Pacifist => Box::new(pacifist::Pacifist),
        WiseElder => Box::new(wise_elder::WiseElder),
        Oracle => Box::new(oracle::Oracle),
        Sandman => Box::new(sandman::Sandman),

        // Not yet ported to the new structure. Listed explicitly (not
        // behind `_`) so the exhaustiveness guarantee above actually holds.
        WolfMan | Thief | Troublemaker | Chemist | SnowWolf | GraveDigger | Augur
        | Arsonist | Spumpkin | Chef | Barkeep => Box::new(Unimplemented(role)),
    }
}
