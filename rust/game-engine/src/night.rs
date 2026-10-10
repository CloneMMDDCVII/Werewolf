//! The night, resolved one region at a time in the legacy order
//! (Werewolf.cs:3088-4190): Arsonist, wolves, Serial Killer, Cultist
//! Hunter, cult, Chemist, Harlot, Guardian Angel, Thief. The Snow Wolf's
//! freeze runs first, in `freeze`; the seers' readings are sent by
//! `reveals` once the deaths are known.
//!
//! Every region goes through one port of `VisitPlayer`
//! (Werewolf.cs:2323-2430), so a player killed earlier in the night is
//! already dead to everyone who visits later, and a Grave Digger's trap,
//! the Serial Killer's door or a burning house catches whoever walks in.
//! Each region tells its players what happened as it goes; the group
//! hears about the deaths afterwards, from `game`.

use crate::messages::{DiggerVisitor, NightNote, Note};
use crate::orchestrator::{grave_digger_fall_chance_percent, AlivePlayer, NarrationEvent};
use crate::reveals::{resolve_night_readings, AugurMemory};
use crate::rng::RngSource;
use crate::role_info::{transform_tells, Cause};
use crate::roles::{NightAction, PlayerId, RoleState};
use shared::{KillMethod, Role, Team};
use std::collections::HashMap;

/// `Settings.AlphaWolfConversionChance`.
pub const ALPHA_WOLF_CONVERSION_CHANCE_PERCENT: u32 = 20;
/// `Settings.HunterKillWolfChanceBase`, plus 20 for each extra wolf.
pub const HUNTER_KILL_WOLF_CHANCE_BASE_PERCENT: u32 = 30;
/// `Settings.ThiefStealChance` (release build).
pub const THIEF_STEAL_CHANCE_PERCENT: u32 = 50;
/// `Settings.HunterConversionChance` and `HunterKillCultChance`.
const HUNTER_CONVERSION_CHANCE_PERCENT: u32 = 50;
const HUNTER_KILL_CULT_CHANCE_PERCENT: u32 = 50;
/// `Settings.ChemistSuccessChance`.
const CHEMIST_SUCCESS_CHANCE_PERCENT: u32 = 50;
/// `Settings.HarlotDiscoverCultChance`.
const HARLOT_DISCOVER_CULT_CHANCE_PERCENT: u32 = 50;
/// A wolf dies visiting a Serial Killer who went out unless this roll
/// fails (Werewolf.cs:2340).
const WOLF_DIES_AT_SERIAL_KILLER_PERCENT: u32 = 80;
/// A Guardian Angel guarding a wolf nobody attacked (Werewolf.cs:2367).
const GUARDIAN_DIES_GUARDING_WOLF_PERCENT: u32 = 50;
/// The Serial Killer's random kill the night after stumbling into a
/// grave (Werewolf.cs:3510).
const SERIAL_KILLER_RANDOM_KILL_PERCENT: u32 = 50;

/// Every `*ConversionChance` in Settings.cs. Roles left out convert at
/// `ConvertToCult`'s default of 100 (Werewolf.cs:2489).
fn cult_conversion_chance_percent(role: Role) -> u32 {
    match role {
        Role::Oracle => 50,
        Role::Sandman => 60,
        Role::WiseElder => 30,
        Role::Pacifist => 80,
        Role::Augur => 40,
        Role::Seer => 40,
        Role::GuardianAngel => 60,
        Role::Detective => 70,
        Role::Cursed => 60,
        Role::Harlot => 70,
        Role::Sorcerer => 40,
        Role::Blacksmith => 75,
        Role::GraveDigger => 30,
        Role::Doppelganger | Role::Thief | Role::Spumpkin => 0,
        _ => 100,
    }
}

/// The legacy wolf vote count: most votes wins, ties go to whoever comes
/// first in the player list (Werewolf.cs:3246-3260). `players` is that
/// list, in order.
pub(crate) fn wolf_vote_winner(
    players: impl Iterator<Item = PlayerId>,
    votes: impl Iterator<Item = PlayerId>,
) -> Option<PlayerId> {
    let votes: Vec<PlayerId> = votes.collect();
    let count = |id: PlayerId| votes.iter().filter(|&&v| v == id).count();
    let mut best: Option<(PlayerId, usize)> = None;
    for id in players {
        let n = count(id);
        if n > 0 && best.is_none_or(|(_, b)| n > b) {
            best = Some((id, n));
        }
    }
    best.map(|(id, _)| id)
}

/// What the night remembers until the next one.
#[derive(Debug, Default, Clone)]
pub struct NightMemory {
    /// Everyone the Arsonist has doused and nobody has cleaned.
    pub doused: Vec<PlayerId>,
    /// Wolves who ate the Drunk: they get no menu next night
    /// (Werewolf.cs:3321, 5316).
    pub drunk: Vec<PlayerId>,
    /// The night the Serial Killer stumbled into a grave (Werewolf.cs:2383).
    pub sk_stumbled: Option<u32>,
    /// The night each convert joined the cult; founders count as night 0
    /// (Werewolf.cs:1993, `DayCult`).
    pub cult_joined: HashMap<PlayerId, u32>,
    /// The roles the Augur has been shown.
    pub augur: AugurMemory,
}

/// Tonight's choices and the facts the regions need.
pub struct NightInput<'a> {
    pub night: u32,
    /// Tonight's actions with their actors, after the freeze.
    pub actions: &'a [(PlayerId, NightAction)],
    /// The wolves' second choice after a Wolf Cub's death: `(wolf, target)`.
    pub wolf_cub_votes: &'a [(PlayerId, PlayerId)],
    pub frozen: Option<PlayerId>,
    /// Whoever the Guardian Angel kept from being frozen (Werewolf.cs:3109).
    pub saved_from_freeze: Option<PlayerId>,
    pub dug_graves: u32,
    pub thief_full: bool,
    /// `ShowRolesDeath`, which decides whether the Chemist pays for killing
    /// the Wise Elder (Werewolf.cs:4278).
    pub reveal: bool,
    /// The pool the roles were dealt from, for the Augur (see
    /// `balancing::balance_with_pool`).
    pub role_pool: &'a [Role],
}

/// One death. `by` is who killed them, or for a Harlot who visited a
/// victim, that victim (the legacy `killer` argument to `KillPlayer`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NightDeath {
    pub victim: PlayerId,
    pub method: KillMethod,
    pub by: Option<PlayerId>,
}

#[derive(Debug, Default)]
pub struct NightOutcome {
    pub deaths: Vec<NightDeath>,
    /// Private notes and transforms, in the order they happened.
    pub events: Vec<NarrationEvent>,
    /// Players the Alpha Wolf bit, who turn at the start of next night.
    pub bitten: Vec<PlayerId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Visit {
    Success,
    VisitorDied,
    Fail,
    AlreadyDead,
}

/// The legacy `KillPlayer` bookkeeping a later region reads.
#[derive(Debug, Clone, Copy)]
struct Death {
    victim: PlayerId,
    method: KillMethod,
    by: Option<PlayerId>,
    /// `KilledByRole`.
    killed_by: Option<Role>,
    /// `DiedByVisitingKiller || DiedByVisitingVictim`.
    by_visiting: bool,
}

/// Resolves tonight. Roles that change mid-night (the Cursed, cult
/// converts, a Thief's steal) are changed in `alive`; deaths are returned
/// for `game` to record and announce.
pub fn resolve_night_sequence(
    input: &NightInput,
    alive: &mut [AlivePlayer],
    states: &mut HashMap<PlayerId, RoleState>,
    memory: &mut NightMemory,
    rng: &mut dyn RngSource,
) -> NightOutcome {
    let ga = input.actions.iter().find_map(|&(actor, action)| match action {
        NightAction::Protect { target } if alive.iter().any(|p| p.id == actor && p.role == Role::GuardianAngel) => {
            Some((actor, target))
        }
        _ => None,
    });
    let start = alive.to_vec();
    let mut night = Night {
        input,
        start,
        alive,
        states,
        memory,
        rng,
        ga,
        deaths: vec![],
        saved: input.saved_from_freeze.into_iter().collect(),
        burning: vec![],
        drunk_tonight: vec![],
        sparked: false,
        outcome: NightOutcome::default(),
    };
    night.arsonist();
    night.wolves();
    night.serial_killer();
    night.cultist_hunter();
    night.cult();
    night.chemist();
    night.harlot();
    night.seers();
    night.guardian_angel();
    night.thief();
    night.witch();
    night.chemist_killed_wise_elder();
    let deaths = night
        .deaths
        .iter()
        .map(|d| NightDeath {
            victim: d.victim,
            method: d.method,
            by: d.by,
        })
        .collect();
    NightOutcome {
        deaths,
        ..night.outcome
    }
}

struct Night<'a, 'b> {
    input: &'b NightInput<'b>,
    /// Everyone alive as the night began, with the roles they had then.
    start: Vec<AlivePlayer>,
    alive: &'a mut [AlivePlayer],
    states: &'a mut HashMap<PlayerId, RoleState>,
    memory: &'a mut NightMemory,
    rng: &'a mut dyn RngSource,
    /// The Guardian Angel and their charge, unless the GA was frozen
    /// (Werewolf.cs:3091, 3152).
    ga: Option<(PlayerId, PlayerId)>,
    deaths: Vec<Death>,
    /// `WasSavedLastNight`.
    saved: Vec<PlayerId>,
    /// Burned by the Arsonist tonight (Werewolf.cs:3202).
    burning: Vec<PlayerId>,
    /// Wolves who ate the Drunk tonight, out of any second kill.
    drunk_tonight: Vec<PlayerId>,
    sparked: bool,
    outcome: NightOutcome,
}

impl Night<'_, '_> {
    fn role(&self, id: PlayerId) -> Option<Role> {
        self.alive.iter().find(|p| p.id == id).map(|p| p.role)
    }

    fn is_dead(&self, id: PlayerId) -> bool {
        self.role(id).is_none() || self.deaths.iter().any(|d| d.victim == id)
    }

    /// Living players, in player-list order.
    fn living(&self, keep: impl Fn(Role) -> bool) -> Vec<PlayerId> {
        self.alive
            .iter()
            .filter(|p| keep(p.role) && !self.is_dead(p.id))
            .map(|p| p.id)
            .collect()
    }

    fn first_living(&self, role: Role) -> Option<PlayerId> {
        self.living(|r| r == role).first().copied()
    }

    fn acts(&self, actor: PlayerId) -> impl Iterator<Item = NightAction> + '_ {
        self.input
            .actions
            .iter()
            .filter(move |(a, _)| *a == actor)
            .map(|&(_, action)| action)
    }

    /// The legacy `Choice != 0 && Choice != -1`: the player went out.
    fn went_out(&self, actor: PlayerId) -> bool {
        self.acts(actor).next().is_some()
    }

    fn not_frozen(&self, id: PlayerId) -> bool {
        self.input.frozen != Some(id)
    }

    fn tell(&mut self, to: PlayerId, note: NightNote) {
        self.note(to, Note::Night(note));
    }

    fn note(&mut self, to: PlayerId, note: Note) {
        self.outcome.events.push(NarrationEvent::Private { to, note });
    }

    fn tell_all(&mut self, to: &[PlayerId], note: NightNote) {
        for &id in to {
            self.tell(id, note);
        }
    }

    /// A percent roll; certain outcomes don't touch the random stream.
    fn roll(&mut self, percent: u32) -> bool {
        match percent {
            0 => false,
            100.. => true,
            _ => self.rng.chance(percent),
        }
    }

    fn kill(
        &mut self,
        victim: PlayerId,
        method: KillMethod,
        by: Option<PlayerId>,
        killed_by: Option<Role>,
        by_visiting: bool,
    ) {
        if !self.is_dead(victim) {
            self.deaths.push(Death {
                victim,
                method,
                by,
                killed_by,
                by_visiting,
            });
        }
    }

    /// Changes a living player's role, with what `Transform` tells them
    /// and their teams (see `role_info`). The dead keep theirs.
    fn transform(&mut self, player: PlayerId, to: Role, cause: Cause) {
        if self.is_dead(player) {
            return;
        }
        if let Some(p) = self.alive.iter_mut().find(|p| p.id == player) {
            let from = p.role;
            p.role = to;
            self.outcome.events.push(NarrationEvent::Transform { player, from, to });
            let living: Vec<AlivePlayer> = self.alive.iter().filter(|p| !self.is_dead(p.id)).copied().collect();
            self.outcome
                .events
                .extend(transform_tells(player, from, to, cause, &living, self.input.thief_full));
        }
    }

    /// `VisitPlayer` (Werewolf.cs:2323-2430).
    fn visit(&mut self, visitor: PlayerId, target: PlayerId) -> Visit {
        let (Some(visitor_role), Some(target_role)) = (self.role(visitor), self.role(target)) else {
            return Visit::AlreadyDead;
        };
        let thief_full = self.input.thief_full;
        let burning = self.burning.contains(&target);
        let dead = self.is_dead(target);
        if dead && !burning && (thief_full || visitor_role != Role::Thief) {
            return Visit::AlreadyDead;
        }
        if visitor_role == Role::SerialKiller && target_role != Role::GraveDigger {
            return if dead { Visit::AlreadyDead } else { Visit::Success };
        }
        if burning {
            if visitor_role == Role::SerialKiller {
                return Visit::AlreadyDead;
            }
            let arsonist = self.alive.iter().find(|p| p.role == Role::Arsonist).map(|p| p.id);
            self.kill(visitor, KillMethod::VisitBurning, arsonist, Some(Role::Arsonist), true);
            return Visit::VisitorDied;
        }
        if target_role == Role::SerialKiller {
            if !visitor_role.is_wolf()
                || !self.went_out(target)
                || !self.not_frozen(target)
                || self.roll(WOLF_DIES_AT_SERIAL_KILLER_PERCENT)
            {
                self.kill(
                    visitor,
                    KillMethod::VisitKiller,
                    Some(target),
                    Some(Role::SerialKiller),
                    true,
                );
                return Visit::VisitorDied;
            }
            return Visit::Success;
        }
        if visitor_role == Role::Thief && !thief_full {
            return Visit::Success;
        }
        if visitor_role == Role::SnowWolf {
            return Visit::Success;
        }
        if target_role.is_wolf() {
            match visitor_role {
                Role::Harlot => {
                    self.kill(visitor, KillMethod::VisitWolf, Some(target), Some(Role::Wolf), true);
                    return Visit::VisitorDied;
                }
                Role::GuardianAngel => {
                    // A GA guarding a wolf who really was attacked never dies.
                    if !self.saved.contains(&target) && self.roll(GUARDIAN_DIES_GUARDING_WOLF_PERCENT) {
                        self.kill(visitor, KillMethod::GuardWolf, Some(target), Some(Role::Wolf), true);
                        return Visit::VisitorDied;
                    }
                    return Visit::Success;
                }
                Role::Thief => return Visit::Fail,
                _ => {}
            }
        }
        if target_role == Role::GraveDigger {
            let dug = self.input.dug_graves;
            if dug < 1 {
                return Visit::Success;
            }
            if visitor_role == Role::SerialKiller {
                self.memory.sk_stumbled = Some(self.input.night);
                self.tell(visitor, NightNote::KillerStumbled(target));
                return Visit::Success;
            }
            if visitor_role == Role::GuardianAngel && self.saved.contains(&target) {
                return Visit::Success;
            }
            let village = visitor_role.team() == Team::Village;
            if self.roll(grave_digger_fall_chance_percent(dug, village)) {
                self.kill(
                    visitor,
                    KillMethod::FallGrave,
                    Some(target),
                    Some(Role::GraveDigger),
                    true,
                );
                let by = match visitor_role {
                    r if r.is_eating_wolf() => Some(DiggerVisitor::Wolf),
                    Role::CultistHunter => Some(DiggerVisitor::CultistHunter),
                    Role::Cultist => Some(DiggerVisitor::Cultist),
                    Role::GuardianAngel => Some(DiggerVisitor::GuardianAngel),
                    Role::Harlot => Some(DiggerVisitor::Harlot),
                    Role::Thief => Some(DiggerVisitor::Thief),
                    Role::Chemist => Some(DiggerVisitor::Chemist),
                    Role::Arsonist => Some(DiggerVisitor::Arsonist),
                    _ => None,
                };
                if let Some(by) = by {
                    self.note(target, Note::FellDigger { visitor, by });
                }
                return Visit::VisitorDied;
            }
            // A lucky Arsonist gets past the traps and finds the digger home.
            if visitor_role != Role::Arsonist {
                return Visit::Fail;
            }
        }
        if visitor_role == Role::Arsonist {
            return Visit::Success;
        }
        let away =
            target_role == Role::Harlot || (target_role == Role::GuardianAngel && !visitor_role.is_eating_wolf());
        if away && self.went_out(target) && self.not_frozen(target) {
            return Visit::Fail;
        }
        Visit::Success
    }

    /// Werewolf.cs:3186-3227.
    fn arsonist(&mut self) {
        let Some(arsonist) = self.first_living(Role::Arsonist) else {
            return;
        };
        let actions: Vec<NightAction> = self.acts(arsonist).collect();
        for action in actions {
            match action {
                NightAction::Detonate => {
                    self.sparked = true;
                    let doused = self.memory.doused.clone();
                    let charge = self.ga.map(|(_, t)| t);
                    for victim in self
                        .living(|r| r != Role::Arsonist)
                        .into_iter()
                        .filter(|p| doused.contains(p))
                    {
                        if charge == Some(victim) {
                            self.saved.push(victim);
                        } else {
                            self.kill(victim, KillMethod::Burn, Some(arsonist), Some(Role::Arsonist), false);
                            self.memory.doused.retain(|&p| p != victim);
                            self.burning.push(victim);
                            self.tell(victim, NightNote::Burn);
                        }
                    }
                }
                NightAction::Douse { target } if self.visit(arsonist, target) == Visit::Success => {
                    if !self.memory.doused.contains(&target) {
                        self.memory.doused.push(target);
                    }
                    self.tell(arsonist, NightNote::Doused(target));
                }
                _ => {}
            }
        }
    }

    fn top_voted(&self, votes: impl Iterator<Item = PlayerId>) -> Option<PlayerId> {
        wolf_vote_winner(self.alive.iter().map(|p| p.id), votes)
    }

    fn vote_wolves(&self, wolves: &[PlayerId]) -> Vec<PlayerId> {
        wolves
            .iter()
            .copied()
            .filter(|w| !self.is_dead(*w) && !self.drunk_tonight.contains(w))
            .collect()
    }

    /// Werewolf.cs:3231-3494.
    fn wolves(&mut self) {
        let wolves = self.living(Role::is_eating_wolf);
        self.memory.drunk.clear();
        if wolves.is_empty() {
            return;
        }
        let first = self.top_voted(self.input.actions.iter().filter_map(|&(actor, action)| match action {
            NightAction::EatVote { target } if wolves.contains(&actor) => Some(target),
            _ => None,
        }));
        let second = self.top_voted(
            self.input
                .wolf_cub_votes
                .iter()
                .filter(|&&(wolf, target)| wolves.contains(&wolf) && Some(target) != first)
                .map(|&(_, target)| target),
        );
        for target in [first, second].into_iter().flatten() {
            let pack = self.vote_wolves(&wolves);
            if pack.is_empty() {
                break; // the pack died or got drunk on the first kill
            }
            let visitor = pack[self.rng.pick_index(pack.len())];
            match self.visit(visitor, target) {
                Visit::Success => self.wolves_attack(target, &wolves, second),
                Visit::Fail => match self.role(target) {
                    Some(Role::Harlot) => self.tell_all(&pack, NightNote::HarlotNotHome(target)),
                    Some(Role::GraveDigger) => self.tell_all(&pack, NightNote::GraveDiggerNotHome(target)),
                    _ => {}
                },
                Visit::VisitorDied => {
                    let pack = self.vote_wolves(&wolves);
                    if self.burning.contains(&target) {
                        self.tell_all(&pack, NightNote::WolvesVisitBurn(target, visitor));
                    } else if self.role(target) == Some(Role::GraveDigger) {
                        self.tell_all(&pack, NightNote::WolfFellWolves(target, visitor));
                    }
                }
                Visit::AlreadyDead => {}
            }
            let pack = self.vote_wolves(&wolves);
            if let Some(gd) = self.spot_grave_digger(pack.first().copied(), Role::Wolf) {
                self.tell_all(&pack, NightNote::WolvesSpotted(gd));
                self.tell(gd, NightNote::WolvesSpottedYou);
            }
        }
    }

    /// One wolf attack that got through the door (Werewolf.cs:3281-3438).
    fn wolves_attack(&mut self, target: PlayerId, wolves: &[PlayerId], second: Option<PlayerId>) {
        let pack = self.vote_wolves(wolves);
        if self.ga.map(|(_, t)| t) == Some(target) {
            self.tell_all(&pack, NightNote::GuardBlockedWolf(target));
            self.saved.push(target);
            return;
        }
        let alpha = pack.iter().copied().find(|&w| self.role(w) == Some(Role::AlphaWolf));
        let bitten = alpha.is_some() && self.roll(ALPHA_WOLF_CONVERSION_CHANCE_PERCENT);
        let eaten = |night: &mut Self| {
            if let Some(alpha) = alpha.filter(|_| bitten) {
                night.bite(target, alpha);
                false
            } else {
                night.eat(target, &pack);
                true
            }
        };
        match self.role(target) {
            Some(Role::Cursed) => self.transform(target, Role::Wolf, Cause::BiteCursed),
            Some(Role::Drunk) => {
                if eaten(self) {
                    let note = match second.filter(|&s| s != target) {
                        Some(second) => NightNote::WolvesEatDrunkBlockSecondKill(target, second),
                        None => NightNote::WolvesEatDrunk(target),
                    };
                    self.tell_all(&pack, note);
                    self.drunk_tonight.extend(&pack);
                    self.memory.drunk = pack.clone();
                }
            }
            Some(Role::Hunter) => {
                let chance = HUNTER_KILL_WOLF_CHANCE_BASE_PERCENT + (pack.len() as u32 - 1) * 20;
                if self.roll(chance) {
                    let shot = pack[self.rng.pick_index(pack.len())];
                    if pack.len() > 1 {
                        self.eat(target, &pack);
                    }
                    self.kill(shot, KillMethod::HunterShot, None, Some(Role::Hunter), true);
                } else {
                    eaten(self);
                }
            }
            Some(Role::WiseElder) if !bitten => {
                let state = self.states.entry(target).or_default();
                if state.primary_used {
                    self.eat(target, &pack);
                } else {
                    state.primary_used = true;
                    self.tell_all(&pack, NightNote::WolvesTriedToEatWiseElder(target));
                    self.tell(target, NightNote::WolvesAteWiseElderPM);
                }
            }
            _ => {
                eaten(self);
            }
        }
    }

    fn eat(&mut self, target: PlayerId, pack: &[PlayerId]) {
        // The Witch's potion (a role legacy never had) stops the wolves too.
        if self
            .input
            .actions
            .iter()
            .any(|(_, a)| *a == NightAction::Heal { target })
        {
            return;
        }
        self.kill(target, KillMethod::Eat, pack.first().copied(), Some(Role::Wolf), false);
        self.tell(target, NightNote::WolvesEatYou);
    }

    /// `BitePlayer` (Werewolf.cs:1943-1950).
    fn bite(&mut self, target: PlayerId, alpha: PlayerId) {
        self.outcome.bitten.push(target);
        let pack = self.living(Role::is_wolf);
        self.tell_all(&pack, NightNote::PlayerBittenWolves(target, alpha));
    }

    /// After each wolf attack and after the Serial Killer, the killers may
    /// spot a Grave Digger who dug last night, on half the fall chance
    /// (Werewolf.cs:3471-3482, 3537-3547). Returns the digger if spotted.
    fn spot_grave_digger(&mut self, spotter: Option<PlayerId>, killed_by: Role) -> Option<PlayerId> {
        let gd = self.first_living(Role::GraveDigger)?;
        let dug = self.input.dug_graves;
        if dug < 1 || !self.roll(grave_digger_fall_chance_percent(dug, true)) {
            return None;
        }
        self.kill(gd, KillMethod::Spotted, spotter, Some(killed_by), true);
        Some(gd)
    }

    /// Werewolf.cs:3497-3549.
    fn serial_killer(&mut self) {
        let Some(sk) = self.first_living(Role::SerialKiller) else {
            return;
        };
        if !self.not_frozen(sk) {
            return;
        }
        let target = self.acts(sk).find_map(|a| match a {
            NightAction::SerialKillVote { target } => Some(target),
            _ => None,
        });
        if let Some(target) = target {
            if self.visit(sk, target) == Visit::Success {
                let mut victim = target;
                let stumbled = self.memory.sk_stumbled.is_some_and(|n| n + 1 == self.input.night);
                let redirected = stumbled && self.roll(SERIAL_KILLER_RANDOM_KILL_PERCENT);
                if redirected {
                    let pool = self.living(|r| r != Role::SerialKiller);
                    victim = pool[self.rng.pick_index(pool.len())];
                    self.visit(sk, victim);
                    self.tell(sk, NightNote::KillerRandomKill(target, victim));
                }
                if self.ga.map(|(_, t)| t) == Some(victim) && self.role(victim) != Some(Role::Harlot) {
                    let note = if redirected {
                        NightNote::GuardBlockedRandomKiller(victim)
                    } else {
                        NightNote::GuardBlockedKiller(victim)
                    };
                    self.tell(sk, note);
                    self.saved.push(victim);
                } else {
                    self.kill(
                        victim,
                        KillMethod::SerialKilled,
                        Some(sk),
                        Some(Role::SerialKiller),
                        false,
                    );
                    self.tell(victim, NightNote::SerialKillerKilledYou);
                }
            }
        }
        if let Some(gd) = self.spot_grave_digger(Some(sk), Role::SerialKiller) {
            self.tell(sk, NightNote::SerialKillerSpotted(gd));
            self.tell(gd, NightNote::SerialKillerSpottedYou);
        }
    }

    /// Werewolf.cs:3556-3587.
    fn cultist_hunter(&mut self) {
        let Some(hunter) = self.first_living(Role::CultistHunter) else {
            return;
        };
        let target = self.acts(hunter).find_map(|a| match a {
            NightAction::Investigate { target } => Some(target),
            _ => None,
        });
        let Some(target) = target.filter(|_| self.not_frozen(hunter)) else {
            return;
        };
        match self.visit(hunter, target) {
            Visit::Success if self.role(target) == Some(Role::Cultist) => {
                self.note(hunter, Note::HunterFindCultist { target });
                self.note(target, Note::CHHuntedYou);
                self.kill(target, KillMethod::Hunt, Some(hunter), Some(Role::CultistHunter), false);
            }
            Visit::Success | Visit::Fail => self.note(hunter, Note::HunterFailedToFind { target }),
            Visit::AlreadyDead => self.note(hunter, Note::HunterVisitDead { target }),
            Visit::VisitorDied => {}
        }
    }

    /// The cultists who act tonight: alive and not frozen.
    fn vote_cult(&self) -> Vec<PlayerId> {
        let frozen = self.input.frozen;
        self.living(|r| r == Role::Cultist)
            .into_iter()
            .filter(|&c| Some(c) != frozen)
            .collect()
    }

    /// Werewolf.cs:3591-3757.
    fn cult(&mut self) {
        let cult = self.vote_cult();
        if cult.is_empty() {
            return;
        }
        // Grouped by target in vote order; ties go to the first vote cast.
        let votes: Vec<PlayerId> = cult
            .iter()
            .filter_map(|&c| {
                self.acts(c).find_map(|a| match a {
                    NightAction::ConvertVote { target } => Some(target),
                    _ => None,
                })
            })
            .collect();
        let mut target = None;
        let mut best = 0;
        for &v in &votes {
            let n = votes.iter().filter(|&&x| x == v).count();
            if n > best {
                best = n;
                target = Some(v);
            }
        }
        let Some(target) = target else { return };
        let joined = |c: &PlayerId| self.memory.cult_joined.get(c).copied().unwrap_or(0);
        // The newest convert visits (`OrderByDescending(DayCult).First()`).
        let newbie = cult
            .iter()
            .copied()
            .rev()
            .max_by_key(joined)
            .expect("the cult is not empty");
        let wolves_went_out = !self.input.wolf_cub_votes.is_empty()
            || self
                .input
                .actions
                .iter()
                .any(|(_, a)| matches!(a, NightAction::EatVote { .. }));
        match self.visit(newbie, target) {
            Visit::Success => match self.role(target).expect("a visited player is in the game") {
                Role::Hunter => {
                    if self.roll(HUNTER_CONVERSION_CHANCE_PERCENT) {
                        self.convert(target, 100, newbie);
                    } else if self.roll(HUNTER_KILL_CULT_CHANCE_PERCENT) {
                        self.kill(newbie, KillMethod::HunterCult, Some(target), Some(Role::Hunter), true);
                        let cult = self.vote_cult();
                        self.tell_all(&cult, NightNote::CultConvertHunter(newbie, target));
                    } else {
                        let cult = self.vote_cult();
                        self.tell_all(&cult, NightNote::CultUnableToConvert(newbie, target));
                        self.tell(target, NightNote::CultAttempt);
                    }
                }
                Role::CultistHunter => {
                    self.kill(newbie, KillMethod::Hunt, Some(target), Some(Role::CultistHunter), true);
                    let cult = self.vote_cult();
                    self.tell_all(&cult, NightNote::CultConvertCultHunter(newbie, target));
                    let left = self.living(|r| r == Role::Cultist).len() as u32;
                    self.tell(target, NightNote::CultHunterKilledCultVisit(newbie, left));
                }
                r if r.is_wolf() => {
                    let home = if r == Role::SnowWolf {
                        !self.went_out(target)
                    } else {
                        !wolves_went_out
                    };
                    if home {
                        self.kill(newbie, KillMethod::VisitWolf, Some(target), Some(Role::Wolf), true);
                        let cult = self.vote_cult();
                        self.tell_all(&cult, NightNote::CultConvertWolf(newbie, target));
                        self.tell(target, NightNote::CultAttempt);
                    } else {
                        self.tell_all(&cult, NightNote::CultVisitEmpty(newbie, target));
                    }
                }
                Role::Arsonist => {
                    if !self.went_out(target) || !self.not_frozen(target) {
                        self.convert(target, 0, newbie);
                    } else {
                        let cult = self.vote_cult();
                        self.tell_all(&cult, NightNote::CultVisitEmpty(newbie, target));
                    }
                }
                r => self.convert(target, cult_conversion_chance_percent(r), newbie),
            },
            Visit::AlreadyDead => {
                if !self.burning.contains(&target) {
                    let cult = self.vote_cult();
                    self.tell_all(&cult, NightNote::CultTargetDead(target));
                }
            }
            Visit::VisitorDied => {
                let cult = self.vote_cult();
                match self.role(target) {
                    Some(Role::SerialKiller) => {
                        self.tell_all(&cult, NightNote::CultConvertSerialKiller(newbie, target))
                    }
                    Some(Role::GraveDigger) => self.tell_all(&cult, NightNote::CultConvertGraveDigger(newbie, target)),
                    _ => {}
                }
            }
            Visit::Fail => {
                let cult = self.vote_cult();
                self.tell_all(&cult, NightNote::CultVisitEmpty(newbie, target));
            }
        }
    }

    /// `ConvertToCult` (Werewolf.cs:2489-2509).
    fn convert(&mut self, target: PlayerId, chance: u32, newbie: PlayerId) {
        if self.roll(chance) {
            self.transform(target, Role::Cultist, Cause::ConvertToCult);
            self.memory.cult_joined.insert(target, self.input.night);
        } else {
            let cultists = self.living(|r| r == Role::Cultist);
            self.tell_all(&cultists, NightNote::CultUnableToConvert(newbie, target));
            self.tell(target, NightNote::CultAttempt);
        }
    }

    /// Werewolf.cs:3792-3838.
    fn chemist(&mut self) {
        let Some(chemist) = self.first_living(Role::Chemist) else {
            return;
        };
        let target = self.acts(chemist).find_map(|a| match a {
            NightAction::Chemistry { target, .. } => Some(target),
            _ => None,
        });
        let Some(target) = target.filter(|_| self.not_frozen(chemist)) else {
            return;
        };
        match self.visit(chemist, target) {
            Visit::Success => {
                // Brewing starts over (Werewolf.cs:3802, 5260-5270).
                self.states.entry(chemist).or_default().primary_used = false;
                if self.roll(CHEMIST_SUCCESS_CHANCE_PERCENT) {
                    self.kill(target, KillMethod::Chemistry, Some(chemist), Some(Role::Chemist), false);
                    self.tell(target, NightNote::ChemistVisitYouSuccess);
                    self.tell(chemist, NightNote::ChemistSuccess(target));
                } else {
                    self.kill(
                        chemist,
                        KillMethod::Chemistry,
                        Some(chemist),
                        Some(Role::Chemist),
                        false,
                    );
                    self.tell(target, NightNote::ChemistVisitYouFail(chemist));
                    self.tell(chemist, NightNote::ChemistFail(target));
                }
            }
            Visit::AlreadyDead => self.tell(chemist, NightNote::ChemistTargetDead(target)),
            Visit::Fail => self.tell(chemist, NightNote::ChemistTargetEmpty(target)),
            Visit::VisitorDied => match self.role(target) {
                Some(Role::SerialKiller) => {
                    self.tell(target, NightNote::ChemistVisitYouSK(chemist));
                    self.tell(chemist, NightNote::ChemistSK(target));
                }
                // The digger was told by the visit itself.
                Some(Role::GraveDigger) => self.tell(chemist, NightNote::ChemistFell(target)),
                _ => {}
            },
        }
    }

    /// Werewolf.cs:3843-3918.
    fn harlot(&mut self) {
        let Some(harlot) = self.first_living(Role::Harlot) else {
            return;
        };
        let target = self.acts(harlot).find_map(|a| match a {
            NightAction::Visit { target, .. } => Some(target),
            _ => None,
        });
        let Some(target) = target.filter(|_| self.not_frozen(harlot)) else {
            return;
        };
        match self.visit(harlot, target) {
            Visit::Success => {
                let discovered =
                    self.role(target) == Some(Role::Cultist) && self.roll(HARLOT_DISCOVER_CULT_CHANCE_PERCENT);
                let note = if discovered {
                    NightNote::HarlotDiscoverCult(target)
                } else {
                    NightNote::HarlotVisitNonWolf(target)
                };
                self.tell(harlot, note);
                if !self.is_dead(target) {
                    self.tell(target, NightNote::HarlotVisitYou);
                }
            }
            Visit::VisitorDied => {
                if !self.burning.contains(&target) {
                    match self.role(target) {
                        Some(r) if r.is_wolf() => self.tell(harlot, NightNote::HarlotFuckWolf(target)),
                        Some(Role::SerialKiller) => self.tell(harlot, NightNote::HarlotFuckKiller(target)),
                        Some(Role::GraveDigger) => self.tell(harlot, NightNote::HarlotFell(target)),
                        _ => {}
                    }
                }
            }
            Visit::AlreadyDead => {
                // Visiting someone the wolves or the Serial Killer killed
                // outright gets her killed too (Werewolf.cs:3893-3897).
                let host = self.deaths.iter().find(|d| d.victim == target).copied();
                match host {
                    Some(d)
                        if !d.by_visiting
                            && d.killed_by
                                .is_some_and(|r| r.is_eating_wolf() || r == Role::SerialKiller) =>
                    {
                        self.kill(harlot, KillMethod::VisitVictim, Some(target), d.killed_by, true);
                    }
                    _ => self.tell(harlot, NightNote::HarlotVisitNotHome(target)),
                }
            }
            Visit::Fail => {
                let note = if self.role(target) == Some(Role::GraveDigger) {
                    NightNote::HarlotVisitDigger(target)
                } else {
                    NightNote::HarlotVisitNotHome(target)
                };
                self.tell(harlot, note);
            }
        }
    }

    /// Werewolf.cs:4065-4125: the GA hears how the night went for their
    /// charge, and scrubs off any kerosene.
    fn guardian_angel(&mut self) {
        let Some((ga, charge)) = self.ga else { return };
        match self.visit(ga, charge) {
            Visit::AlreadyDead | Visit::Success => {
                let saved = self.saved.contains(&charge);
                let doused = self.memory.doused.contains(&charge);
                if doused {
                    self.memory.doused.retain(|&p| p != charge);
                }
                if saved && doused && self.sparked {
                    self.tell(ga, NightNote::GuardSavedFromFire(charge));
                    self.tell(charge, NightNote::GuardSavedYouFromFire);
                } else if saved {
                    self.tell(ga, NightNote::GuardSaved(charge));
                    self.tell(charge, NightNote::GuardSavedYou);
                } else if doused {
                    self.tell(ga, NightNote::CleanDoused(charge));
                }
                let died = self.deaths.iter().any(|d| d.victim == charge);
                if !saved && !died && !doused {
                    self.tell(ga, NightNote::GuardNoAttack(charge));
                }
            }
            Visit::Fail => self.tell(ga, NightNote::GuardEmptyHouse(charge)),
            Visit::VisitorDied => match self.role(charge) {
                Some(r) if r.is_wolf() => self.tell(ga, NightNote::GuardWolf),
                Some(Role::SerialKiller) => self.tell(ga, NightNote::GuardKiller),
                Some(Role::GraveDigger) => self.tell(ga, NightNote::GAFell(charge)),
                _ => {}
            },
        }
    }

    /// Werewolf.cs:4132-4190.
    fn thief(&mut self) {
        let Some(thief) = self.first_living(Role::Thief) else {
            return;
        };
        let picked = self.acts(thief).find_map(|a| match a {
            NightAction::StealRole { target, .. } => Some(target),
            _ => None,
        });
        if !self.input.thief_full {
            // The plain Thief steals on night one, frozen or not, from a
            // random player if they picked nobody.
            if self.input.night != 1 {
                return;
            }
            let target = match picked {
                Some(target) => target,
                None => {
                    let Some(target) = self.random_other(thief) else { return };
                    self.note(thief, Note::ThiefStealChosen { target });
                    target
                }
            };
            if self.visit(thief, target) == Visit::Success {
                self.steal(thief, target);
            }
            return;
        }
        let Some(target) = picked.filter(|_| self.not_frozen(thief)) else {
            return;
        };
        match self.visit(thief, target) {
            Visit::Success => {
                let lands = self.roll(THIEF_STEAL_CHANCE_PERCENT);
                let role = self.role(target).expect("a visited player is in the game");
                if lands && !role.is_wolf() && role != Role::Cultist {
                    self.steal(thief, target);
                } else {
                    self.note(thief, Note::ThiefStealFailed { target });
                }
            }
            Visit::VisitorDied => {
                if !self.burning.contains(&target) && self.role(target) == Some(Role::GraveDigger) {
                    self.note(thief, Note::ThiefFell { target });
                } else if self.role(target) == Some(Role::SerialKiller) {
                    self.note(thief, Note::StealKiller);
                }
            }
            Visit::Fail => self.note(thief, Note::ThiefStealFailed { target }),
            Visit::AlreadyDead => self.note(thief, Note::ThiefStealDead { target }),
        }
    }

    /// `ChooseRandomPlayerId(thief, false)`: anyone else still alive.
    fn random_other(&mut self, player: PlayerId) -> Option<PlayerId> {
        let pool: Vec<PlayerId> = self.living(|_| true).into_iter().filter(|&p| p != player).collect();
        (!pool.is_empty()).then(|| pool[self.rng.pick_index(pool.len())])
    }

    /// `StealRole` (Werewolf.cs:2432-2487): the Thief takes the target's
    /// role, used abilities and bullets, and the target becomes a Villager,
    /// or the new Thief in full mode.
    fn steal(&mut self, thief: PlayerId, mut target: PlayerId) {
        if self.is_dead(target) {
            self.note(thief, Note::ThiefStealDead { target });
            let Some(other) = self.random_other(thief) else { return };
            target = other;
            self.note(thief, Note::ThiefStealChosen { target });
        }
        if self.visit(thief, target) != Visit::Success {
            return;
        }
        let Some(stolen) = self.role(target) else { return };
        let victim_becomes = if self.input.thief_full {
            Role::Thief
        } else {
            Role::Villager
        };
        self.transform(target, victim_becomes, Cause::ThiefStolen);
        self.note(thief, Note::ThiefStoleRole { target });
        self.transform(thief, stolen, Cause::ThiefSteal { from: target });
        let robbed = if self.input.thief_full {
            Note::ThiefStoleYourRoleThief
        } else {
            Note::ThiefStoleYourRoleVillager
        };
        self.note(target, robbed);
        let taken = self.states.remove(&target).unwrap_or_default();
        self.states.insert(thief, taken);
    }

    /// The Witch's poison, a role this port added.
    fn witch(&mut self) {
        let poisoned: Vec<PlayerId> = self
            .input
            .actions
            .iter()
            .filter_map(|(_, a)| match a {
                NightAction::Poison { target } => Some(*target),
                _ => None,
            })
            .collect();
        for target in poisoned {
            self.kill(target, KillMethod::Poison, None, None, false);
        }
    }

    /// The Seer, Sorcerer, Fool, Oracle and Augur, after the Harlot and
    /// before the Guardian Angel (Werewolf.cs, `NightCycle`), so a reader
    /// killed earlier tonight learns nothing and the readings arrive
    /// before the morning's death notices.
    fn seers(&mut self) {
        let survivors: Vec<AlivePlayer> = self.alive.iter().filter(|p| !self.is_dead(p.id)).copied().collect();
        let readings = resolve_night_readings(
            self.input.actions,
            &self.start,
            &survivors,
            self.input.frozen,
            self.input.role_pool,
            &mut self.memory.augur,
            self.rng,
        );
        self.outcome.events.extend(readings);
    }

    /// Werewolf.cs:4276-4281: with roles shown, a Chemist who killed the
    /// Wise Elder loses their role (the death line says so).
    fn chemist_killed_wise_elder(&mut self) {
        if !self.input.reveal {
            return;
        }
        let killed_elder = self.deaths.iter().any(|d| {
            d.method == KillMethod::Chemistry && d.by != Some(d.victim) && self.role(d.victim) == Some(Role::WiseElder)
        });
        if let Some(chemist) = self.first_living(Role::Chemist).filter(|_| killed_elder) {
            self.transform(chemist, Role::Villager, Cause::KillElder);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::ScriptedRngSource;

    const P1: PlayerId = PlayerId(1);
    const P2: PlayerId = PlayerId(2);
    const P3: PlayerId = PlayerId(3);
    const P4: PlayerId = PlayerId(4);
    const P5: PlayerId = PlayerId(5);

    /// One night to resolve: players are P1, P2, ... in the order given.
    struct Scene {
        alive: Vec<AlivePlayer>,
        actions: Vec<(PlayerId, NightAction)>,
        cub: Vec<(PlayerId, PlayerId)>,
        night: u32,
        frozen: Option<PlayerId>,
        dug: u32,
        thief_full: bool,
        memory: NightMemory,
        states: HashMap<PlayerId, RoleState>,
    }

    fn scene(roles: &[Role]) -> Scene {
        Scene {
            alive: roles
                .iter()
                .enumerate()
                .map(|(i, &role)| AlivePlayer {
                    id: PlayerId(i as u64 + 1),
                    role,
                })
                .collect(),
            actions: vec![],
            cub: vec![],
            night: 1,
            frozen: None,
            dug: 0,
            thief_full: false,
            memory: NightMemory::default(),
            states: HashMap::new(),
        }
    }

    impl Scene {
        fn act(mut self, actor: PlayerId, action: NightAction) -> Self {
            self.actions.push((actor, action));
            self
        }

        fn eat(self, wolf: PlayerId, target: PlayerId) -> Self {
            self.act(wolf, NightAction::EatVote { target })
        }

        fn run(&mut self, rng: &mut ScriptedRngSource) -> NightOutcome {
            let input = NightInput {
                night: self.night,
                actions: &self.actions,
                wolf_cub_votes: &self.cub,
                frozen: self.frozen,
                saved_from_freeze: None,
                dug_graves: self.dug,
                thief_full: self.thief_full,
                reveal: true,
                role_pool: &[],
            };
            resolve_night_sequence(&input, &mut self.alive, &mut self.states, &mut self.memory, rng)
        }

        fn role(&self, id: PlayerId) -> Role {
            self.alive.iter().find(|p| p.id == id).unwrap().role
        }
    }

    fn rolls(script: &[Roll]) -> ScriptedRngSource {
        script.iter().fold(ScriptedRngSource::new(), |rng, roll| match *roll {
            Roll::Chance(c) => rng.with_chance(c),
            Roll::Pick(i) => rng.with_index(i),
        })
    }

    #[derive(Clone, Copy)]
    enum Roll {
        Chance(bool),
        Pick(usize),
    }
    use Roll::{Chance, Pick};

    fn deaths(out: &NightOutcome) -> Vec<(PlayerId, KillMethod)> {
        out.deaths.iter().map(|d| (d.victim, d.method)).collect()
    }

    fn notes(out: &NightOutcome, to: PlayerId) -> Vec<Note> {
        out.events
            .iter()
            .filter_map(|e| match e {
                NarrationEvent::Private { to: t, note } if *t == to => Some(*note),
                _ => None,
            })
            .collect()
    }

    fn night(note: NightNote) -> Note {
        Note::Night(note)
    }

    // --- Wolves ---

    #[test]
    fn a_tied_wolf_vote_goes_to_whoever_is_listed_first() {
        let mut s = scene(&[Role::Wolf, Role::Wolf, Role::Villager, Role::Villager])
            .eat(P1, P4)
            .eat(P2, P3);
        let out = s.run(&mut rolls(&[Pick(0)]));
        assert_eq!(deaths(&out), vec![(P3, KillMethod::Eat)]);
        assert_eq!(notes(&out, P3), vec![night(NightNote::WolvesEatYou)]);
    }

    #[test]
    fn the_guardian_angel_turns_the_wolves_away() {
        let mut s = scene(&[Role::Wolf, Role::GuardianAngel, Role::Villager])
            .eat(P1, P3)
            .act(P2, NightAction::Protect { target: P3 });
        let out = s.run(&mut rolls(&[Pick(0)]));
        assert_eq!(deaths(&out), vec![]);
        assert_eq!(notes(&out, P1), vec![night(NightNote::GuardBlockedWolf(P3))]);
        assert_eq!(notes(&out, P2), vec![night(NightNote::GuardSaved(P3))]);
        assert_eq!(notes(&out, P3), vec![night(NightNote::GuardSavedYou)]);
    }

    #[test]
    fn eating_the_drunk_cancels_the_second_kill_and_the_next_night() {
        let mut s = scene(&[Role::Wolf, Role::Drunk, Role::Villager, Role::Villager]).eat(P1, P2);
        s.cub = vec![(P1, P3)];
        let out = s.run(&mut rolls(&[Pick(0)]));
        assert_eq!(deaths(&out), vec![(P2, KillMethod::Eat)]);
        assert_eq!(
            notes(&out, P1),
            vec![night(NightNote::WolvesEatDrunkBlockSecondKill(P2, P3))]
        );
        assert_eq!(s.memory.drunk, vec![P1]);
    }

    #[test]
    fn the_wise_elder_survives_the_first_attack_only() {
        let mut s = scene(&[Role::Wolf, Role::WiseElder, Role::Villager]).eat(P1, P2);
        let out = s.run(&mut rolls(&[Pick(0)]));
        assert_eq!(deaths(&out), vec![]);
        assert_eq!(notes(&out, P1), vec![night(NightNote::WolvesTriedToEatWiseElder(P2))]);
        assert_eq!(notes(&out, P2), vec![night(NightNote::WolvesAteWiseElderPM)]);
        let out = s.run(&mut rolls(&[Pick(0)]));
        assert_eq!(deaths(&out), vec![(P2, KillMethod::Eat)]);
    }

    #[test]
    fn the_cursed_joins_the_pack() {
        let mut s = scene(&[Role::Wolf, Role::Cursed, Role::Villager]).eat(P1, P2);
        let out = s.run(&mut rolls(&[Pick(0)]));
        assert_eq!(deaths(&out), vec![]);
        assert_eq!(s.role(P2), Role::Wolf);
    }

    #[test]
    fn the_alpha_wolf_bites_instead_of_killing() {
        let mut s = scene(&[Role::AlphaWolf, Role::Wolf, Role::Villager, Role::Villager])
            .eat(P1, P3)
            .eat(P2, P3);
        let out = s.run(&mut rolls(&[Pick(1), Chance(true)]));
        assert_eq!(deaths(&out), vec![]);
        assert_eq!(out.bitten, vec![P3]);
        assert_eq!(notes(&out, P2), vec![night(NightNote::PlayerBittenWolves(P3, P1))]);
    }

    #[test]
    fn a_hunter_shoots_back_and_dies_only_to_a_pack() {
        let mut s = scene(&[Role::Wolf, Role::Hunter, Role::Villager]).eat(P1, P2);
        let out = s.run(&mut rolls(&[Pick(0), Chance(true), Pick(0)]));
        assert_eq!(deaths(&out), vec![(P1, KillMethod::HunterShot)]);

        let mut s = scene(&[Role::Wolf, Role::Wolf, Role::Hunter, Role::Villager])
            .eat(P1, P3)
            .eat(P2, P3);
        let out = s.run(&mut rolls(&[Pick(0), Chance(true), Pick(1)]));
        assert_eq!(deaths(&out), vec![(P3, KillMethod::Eat), (P2, KillMethod::HunterShot)]);
    }

    #[test]
    fn a_wolf_visiting_the_serial_killer_usually_dies() {
        let mut s = scene(&[Role::Wolf, Role::SerialKiller, Role::Villager])
            .eat(P1, P2)
            .act(P2, NightAction::SerialKillVote { target: P3 });
        let out = s.run(&mut rolls(&[Pick(0), Chance(true)]));
        assert_eq!(
            deaths(&out),
            vec![(P1, KillMethod::VisitKiller), (P3, KillMethod::SerialKilled)]
        );
    }

    #[test]
    fn a_harlot_who_went_out_is_not_home_for_the_wolves() {
        let mut s = scene(&[Role::Wolf, Role::Harlot, Role::Villager]).eat(P1, P2).act(
            P2,
            NightAction::Visit {
                visitor: P2,
                target: P3,
            },
        );
        let out = s.run(&mut rolls(&[Pick(0)]));
        assert_eq!(deaths(&out), vec![]);
        assert_eq!(notes(&out, P1), vec![night(NightNote::HarlotNotHome(P2))]);
        assert_eq!(notes(&out, P2), vec![night(NightNote::HarlotVisitNonWolf(P3))]);
    }

    #[test]
    fn wolves_can_fall_into_a_grave_or_spot_the_digger() {
        let mut s = scene(&[Role::Wolf, Role::Wolf, Role::GraveDigger, Role::Villager])
            .eat(P1, P3)
            .eat(P2, P3);
        s.dug = 2;
        let out = s.run(&mut rolls(&[Pick(0), Chance(true), Chance(false)]));
        assert_eq!(deaths(&out), vec![(P1, KillMethod::FallGrave)]);
        assert_eq!(notes(&out, P2), vec![night(NightNote::WolfFellWolves(P3, P1))]);
        assert_eq!(
            notes(&out, P3),
            vec![Note::FellDigger {
                visitor: P1,
                by: DiggerVisitor::Wolf
            }]
        );

        let mut s = scene(&[Role::Wolf, Role::GraveDigger, Role::Villager]).eat(P1, P3);
        s.dug = 1;
        let out = s.run(&mut rolls(&[Pick(0), Chance(true)]));
        assert_eq!(deaths(&out), vec![(P3, KillMethod::Eat), (P2, KillMethod::Spotted)]);
        assert_eq!(out.deaths[1].by, Some(P1));
        assert_eq!(notes(&out, P1), vec![night(NightNote::WolvesSpotted(P2))]);
    }

    // --- Arsonist ---

    #[test]
    fn a_spark_burns_the_doused_and_the_guardian_angel_saves_one() {
        let mut s = scene(&[
            Role::Arsonist,
            Role::GuardianAngel,
            Role::Villager,
            Role::Villager,
            Role::Wolf,
        ])
        .act(P1, NightAction::Detonate)
        .act(P2, NightAction::Protect { target: P3 })
        .eat(P5, P4);
        s.memory.doused = vec![P3, P4];
        let out = s.run(&mut rolls(&[Pick(0)]));
        assert_eq!(
            deaths(&out),
            vec![(P4, KillMethod::Burn), (P5, KillMethod::VisitBurning)]
        );
        assert_eq!(notes(&out, P4), vec![night(NightNote::Burn)]);
        assert_eq!(notes(&out, P2), vec![night(NightNote::GuardSavedFromFire(P3))]);
        assert!(s.memory.doused.is_empty());
    }

    #[test]
    fn the_guardian_angel_cleans_kerosene_off_a_quiet_charge() {
        let mut s =
            scene(&[Role::Arsonist, Role::GuardianAngel, Role::Villager]).act(P2, NightAction::Protect { target: P3 });
        s.memory.doused = vec![P3];
        let out = s.run(&mut rolls(&[]));
        assert_eq!(notes(&out, P2), vec![night(NightNote::CleanDoused(P3))]);
        assert!(s.memory.doused.is_empty());
    }

    // --- Serial Killer ---

    #[test]
    fn the_guardian_angel_stops_the_serial_killer() {
        let mut s = scene(&[Role::SerialKiller, Role::GuardianAngel, Role::Villager])
            .act(P1, NightAction::SerialKillVote { target: P3 })
            .act(P2, NightAction::Protect { target: P3 });
        let out = s.run(&mut rolls(&[]));
        assert_eq!(deaths(&out), vec![]);
        assert_eq!(notes(&out, P1), vec![night(NightNote::GuardBlockedKiller(P3))]);
    }

    #[test]
    fn a_stumble_into_a_grave_can_send_the_killer_elsewhere_next_night() {
        let mut s = scene(&[Role::SerialKiller, Role::GraveDigger, Role::Villager, Role::Villager])
            .act(P1, NightAction::SerialKillVote { target: P2 });
        s.dug = 1;
        let out = s.run(&mut rolls(&[]));
        assert_eq!(deaths(&out), vec![(P2, KillMethod::SerialKilled)]);
        assert_eq!(notes(&out, P1), vec![night(NightNote::KillerStumbled(P2))]);
        assert_eq!(s.memory.sk_stumbled, Some(1));

        s.night = 2;
        s.dug = 0;
        s.alive.retain(|p| p.id != P2);
        s.actions = vec![(P1, NightAction::SerialKillVote { target: P3 })];
        let out = s.run(&mut rolls(&[Chance(true), Pick(1)]));
        assert_eq!(deaths(&out), vec![(P4, KillMethod::SerialKilled)]);
        assert_eq!(notes(&out, P1), vec![night(NightNote::KillerRandomKill(P3, P4))]);
    }

    // --- Cult ---

    #[test]
    fn the_newest_cultist_visits_and_dies_at_the_cultist_hunter() {
        let mut s = scene(&[Role::Cultist, Role::Cultist, Role::CultistHunter, Role::Villager])
            .act(P1, NightAction::ConvertVote { target: P3 })
            .act(P2, NightAction::ConvertVote { target: P3 });
        s.memory.cult_joined.insert(P2, 1);
        s.night = 2;
        let out = s.run(&mut rolls(&[]));
        assert_eq!(deaths(&out), vec![(P2, KillMethod::Hunt)]);
        assert_eq!(notes(&out, P1), vec![night(NightNote::CultConvertCultHunter(P2, P3))]);
        assert_eq!(
            notes(&out, P3),
            vec![night(NightNote::CultHunterKilledCultVisit(P2, 1))]
        );
    }

    #[test]
    fn a_role_that_resists_is_told_about_the_attempt() {
        let mut s =
            scene(&[Role::Cultist, Role::Seer, Role::Villager]).act(P1, NightAction::ConvertVote { target: P2 });
        let out = s.run(&mut rolls(&[Chance(false)]));
        assert_eq!(notes(&out, P1), vec![night(NightNote::CultUnableToConvert(P1, P2))]);
        assert_eq!(notes(&out, P2), vec![night(NightNote::CultAttempt)]);
        let out = s.run(&mut rolls(&[Chance(true)]));
        assert_eq!(s.role(P2), Role::Cultist);
        assert!(out.events.contains(&NarrationEvent::Transform {
            player: P2,
            from: Role::Seer,
            to: Role::Cultist
        }));
        assert_eq!(s.memory.cult_joined.get(&P2), Some(&1));
    }

    #[test]
    fn the_cult_finds_a_dead_target_or_wolves_out_hunting() {
        let mut s = scene(&[Role::Cultist, Role::Wolf, Role::Villager])
            .act(P1, NightAction::ConvertVote { target: P3 })
            .eat(P2, P3);
        let out = s.run(&mut rolls(&[Pick(0)]));
        assert_eq!(notes(&out, P1), vec![night(NightNote::CultTargetDead(P3))]);

        let mut s = scene(&[Role::Cultist, Role::Wolf, Role::Villager])
            .act(P1, NightAction::ConvertVote { target: P2 })
            .eat(P2, P3);
        let out = s.run(&mut rolls(&[Pick(0)]));
        assert_eq!(notes(&out, P1), vec![night(NightNote::CultVisitEmpty(P1, P2))]);

        let mut s =
            scene(&[Role::Cultist, Role::Wolf, Role::Villager]).act(P1, NightAction::ConvertVote { target: P2 });
        let out = s.run(&mut rolls(&[]));
        assert_eq!(deaths(&out), vec![(P1, KillMethod::VisitWolf)]);
        assert_eq!(notes(&out, P2), vec![night(NightNote::CultAttempt)]);
    }

    // --- Chemist ---

    #[test]
    fn the_chemist_kills_or_dies_and_brews_again() {
        let chemistry = NightAction::Chemistry {
            chemist: P1,
            target: P2,
        };
        let mut s = scene(&[Role::Chemist, Role::Villager, Role::Wolf]).act(P1, chemistry);
        s.states.insert(
            P1,
            RoleState {
                primary_used: true,
                ..RoleState::default()
            },
        );
        let out = s.run(&mut rolls(&[Chance(true)]));
        assert_eq!(deaths(&out), vec![(P2, KillMethod::Chemistry)]);
        assert_eq!(notes(&out, P1), vec![night(NightNote::ChemistSuccess(P2))]);
        assert!(!s.states[&P1].primary_used, "the next night is spent brewing");

        let mut s = scene(&[Role::Chemist, Role::Villager, Role::Wolf]).act(P1, chemistry);
        let out = s.run(&mut rolls(&[Chance(false)]));
        assert_eq!(deaths(&out), vec![(P1, KillMethod::Chemistry)]);
        assert_eq!(notes(&out, P2), vec![night(NightNote::ChemistVisitYouFail(P1))]);
    }

    #[test]
    fn the_chemist_finds_an_empty_house() {
        let mut s = scene(&[Role::Chemist, Role::Harlot, Role::Villager])
            .act(
                P1,
                NightAction::Chemistry {
                    chemist: P1,
                    target: P2,
                },
            )
            .act(
                P2,
                NightAction::Visit {
                    visitor: P2,
                    target: P3,
                },
            );
        let out = s.run(&mut rolls(&[]));
        assert_eq!(notes(&out, P1), vec![night(NightNote::ChemistTargetEmpty(P2))]);
    }

    // --- Harlot ---

    #[test]
    fn a_harlot_visiting_the_wolves_victim_dies_with_them() {
        let mut s = scene(&[Role::Wolf, Role::Harlot, Role::Villager]).eat(P1, P3).act(
            P2,
            NightAction::Visit {
                visitor: P2,
                target: P3,
            },
        );
        let out = s.run(&mut rolls(&[Pick(0)]));
        assert_eq!(deaths(&out), vec![(P3, KillMethod::Eat), (P2, KillMethod::VisitVictim)]);
        assert_eq!(out.deaths[1].by, Some(P3));
    }

    #[test]
    fn a_harlot_visiting_a_wolf_dies() {
        let mut s = scene(&[Role::Wolf, Role::Harlot, Role::Villager]).act(
            P2,
            NightAction::Visit {
                visitor: P2,
                target: P1,
            },
        );
        let out = s.run(&mut rolls(&[]));
        assert_eq!(deaths(&out), vec![(P2, KillMethod::VisitWolf)]);
        assert_eq!(notes(&out, P2), vec![night(NightNote::HarlotFuckWolf(P1))]);
    }

    #[test]
    fn a_harlot_may_discover_a_cultist() {
        let mut s = scene(&[Role::Cultist, Role::Harlot, Role::Villager]).act(
            P2,
            NightAction::Visit {
                visitor: P2,
                target: P1,
            },
        );
        let out = s.run(&mut rolls(&[Chance(true)]));
        assert_eq!(notes(&out, P2), vec![night(NightNote::HarlotDiscoverCult(P1))]);
        assert_eq!(notes(&out, P1), vec![night(NightNote::HarlotVisitYou)]);
    }

    // --- Guardian Angel ---

    #[test]
    fn a_guardian_angel_guarding_an_unattacked_wolf_may_die() {
        let mut s =
            scene(&[Role::Wolf, Role::GuardianAngel, Role::Villager]).act(P2, NightAction::Protect { target: P1 });
        let out = s.run(&mut rolls(&[Chance(true)]));
        assert_eq!(deaths(&out), vec![(P2, KillMethod::GuardWolf)]);
        assert_eq!(notes(&out, P2), vec![night(NightNote::GuardWolf)]);
        let mut s =
            scene(&[Role::Wolf, Role::GuardianAngel, Role::Villager]).act(P2, NightAction::Protect { target: P1 });
        let out = s.run(&mut rolls(&[Chance(false)]));
        assert_eq!(notes(&out, P2), vec![night(NightNote::GuardNoAttack(P1))]);
    }

    #[test]
    fn a_guardian_angel_finds_a_harlot_out() {
        let mut s = scene(&[Role::GuardianAngel, Role::Harlot, Role::Villager])
            .act(P1, NightAction::Protect { target: P2 })
            .act(
                P2,
                NightAction::Visit {
                    visitor: P2,
                    target: P3,
                },
            );
        let out = s.run(&mut rolls(&[]));
        assert_eq!(notes(&out, P1), vec![night(NightNote::GuardEmptyHouse(P2))]);
    }

    // --- Thief and Cultist Hunter ---

    fn steal(s: Scene) -> Scene {
        s.act(P1, NightAction::StealRole { thief: P1, target: P2 })
    }

    fn full(mut s: Scene) -> Scene {
        s.thief_full = true;
        s
    }

    #[test]
    fn a_full_steal_lands_on_a_good_roll() {
        let mut s = full(steal(scene(&[Role::Thief, Role::Seer])));
        let out = s.run(&mut rolls(&[Chance(true)]));
        assert_eq!((s.role(P1), s.role(P2)), (Role::Seer, Role::Thief));
        assert_eq!(notes(&out, P2), vec![Note::ThiefStoleYourRoleThief]);
        let mut s = full(steal(scene(&[Role::Thief, Role::Seer])));
        let out = s.run(&mut rolls(&[Chance(false)]));
        assert_eq!(notes(&out, P1), vec![Note::ThiefStealFailed { target: P2 }]);
    }

    #[test]
    fn wolves_and_cultists_cannot_be_stolen_from() {
        for role in [Role::Wolf, Role::SnowWolf] {
            let mut s = full(steal(scene(&[Role::Thief, role])));
            let out = s.run(&mut rolls(&[]));
            assert_eq!(notes(&out, P1), vec![Note::ThiefStealFailed { target: P2 }]);
        }
        let mut s = full(steal(scene(&[Role::Thief, Role::Cultist])));
        let out = s.run(&mut rolls(&[Chance(true)]));
        assert_eq!(notes(&out, P1), vec![Note::ThiefStealFailed { target: P2 }]);
    }

    #[test]
    fn the_serial_killer_kills_a_visiting_thief_or_hunter() {
        let mut s = full(steal(scene(&[Role::Thief, Role::SerialKiller])));
        let out = s.run(&mut rolls(&[]));
        assert_eq!(deaths(&out), vec![(P1, KillMethod::VisitKiller)]);
        assert_eq!(notes(&out, P1), vec![Note::StealKiller]);

        let mut s = scene(&[Role::CultistHunter, Role::SerialKiller]).act(P1, NightAction::Investigate { target: P2 });
        let out = s.run(&mut rolls(&[]));
        assert_eq!(
            (deaths(&out), notes(&out, P1)),
            (vec![(P1, KillMethod::VisitKiller)], vec![])
        );
    }

    #[test]
    fn a_grave_digger_can_trap_the_thief() {
        let mut s = full(steal(scene(&[Role::Thief, Role::GraveDigger])));
        s.dug = 1;
        let out = s.run(&mut rolls(&[Chance(true)]));
        assert_eq!(deaths(&out), vec![(P1, KillMethod::FallGrave)]);
        assert_eq!(notes(&out, P1), vec![Note::ThiefFell { target: P2 }]);
        assert_eq!(
            notes(&out, P2),
            vec![Note::FellDigger {
                visitor: P1,
                by: DiggerVisitor::Thief
            }]
        );
    }

    #[test]
    fn the_plain_thief_picks_someone_at_random_when_they_chose_nobody() {
        let mut s = scene(&[Role::Thief, Role::Seer, Role::Villager]);
        let out = s.run(&mut rolls(&[Pick(0)]));
        assert_eq!(
            notes(&out, P1)[..2],
            [
                Note::ThiefStealChosen { target: P2 },
                Note::ThiefStoleRole { target: P2 }
            ]
        );
        assert_eq!(s.role(P2), Role::Villager);
    }

    #[test]
    fn the_cultist_hunter_finds_a_cultist_or_nothing() {
        let mut s = scene(&[Role::CultistHunter, Role::Cultist]).act(P1, NightAction::Investigate { target: P2 });
        let out = s.run(&mut rolls(&[]));
        assert_eq!(deaths(&out), vec![(P2, KillMethod::Hunt)]);
        assert_eq!(notes(&out, P1), vec![Note::HunterFindCultist { target: P2 }]);

        let mut s = scene(&[Role::CultistHunter, Role::Seer]).act(P1, NightAction::Investigate { target: P2 });
        let out = s.run(&mut rolls(&[]));
        assert_eq!(notes(&out, P1), vec![Note::HunterFailedToFind { target: P2 }]);
    }
}
