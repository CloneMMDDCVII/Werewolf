//! What players are told privately about their role: when roles are dealt
//! (`NotifyRoles`/`GetRoleInfo`, Werewolf.cs:1612-1711) and whenever a
//! role changes (`Transform`'s messages, Werewolf.cs:1963-2260). See
//! features/role-info.feature.

use crate::messages::{Message, MessageArg};
use crate::orchestrator::{AlivePlayer, NarrationEvent};
use crate::roles::PlayerId;
use shared::Role;

/// The legacy `WolfRoles` (Werewolf.cs:48): the wolves that eat, which
/// leave out the Snow Wolf.
fn eats(role: Role) -> bool {
    matches!(role, Role::Wolf | Role::AlphaWolf | Role::WolfCub | Role::Lycan)
}

fn ids(alive: &[AlivePlayer], keep: impl Fn(&AlivePlayer) -> bool) -> Vec<PlayerId> {
    alive.iter().filter(|p| keep(p)).map(|p| p.id).collect()
}

fn holder(alive: &[AlivePlayer], role: Role) -> Option<PlayerId> {
    alive.iter().find(|p| p.role == role).map(|p| p.id)
}

fn names(list: Vec<PlayerId>) -> MessageArg {
    MessageArg::Players(list)
}

fn tell(to: PlayerId, message: Message) -> NarrationEvent {
    NarrationEvent::Tell { to, message }
}

fn tell_all(to: &[PlayerId], message: Message) -> Vec<NarrationEvent> {
    to.iter().map(|&id| tell(id, message.clone())).collect()
}

/// `RoleInfo{role}`, the key every role's description is under.
fn role_info_key(role: Role) -> &'static str {
    match role {
        Role::Villager => "RoleInfoVillager",
        Role::Drunk => "RoleInfoDrunk",
        Role::Harlot => "RoleInfoHarlot",
        Role::Seer => "RoleInfoSeer",
        Role::Traitor => "RoleInfoTraitor",
        Role::GuardianAngel => "RoleInfoGuardianAngel",
        Role::Detective => "RoleInfoDetective",
        Role::Wolf => "RoleInfoWolf",
        Role::Cursed => "RoleInfoCursed",
        Role::Gunner => "RoleInfoGunner",
        Role::Tanner => "RoleInfoTanner",
        // The Fool is told they're the Seer (Werewolf.cs:1650).
        Role::Fool => "RoleInfoSeer",
        Role::WildChild => "RoleInfoWildChild",
        Role::Beholder => "RoleInfoBeholder",
        Role::ApprenticeSeer => "RoleInfoApprenticeSeer",
        Role::Cultist => "RoleInfoCultist",
        Role::CultistHunter => "RoleInfoCultistHunter",
        Role::Mason => "RoleInfoMason",
        Role::Doppelganger => "RoleInfoDoppelganger",
        Role::Cupid => "RoleInfoCupid",
        Role::Hunter => "RoleInfoHunter",
        Role::SerialKiller => "RoleInfoSerialKiller",
        Role::Sorcerer => "RoleInfoSorcerer",
        Role::AlphaWolf => "RoleInfoAlphaWolf",
        Role::WolfCub => "RoleInfoWolfCub",
        Role::Blacksmith => "RoleInfoBlacksmith",
        Role::ClumsyGuy => "RoleInfoClumsyGuy",
        Role::Mayor => "RoleInfoMayor",
        Role::Prince => "RoleInfoPrince",
        Role::Lycan => "RoleInfoLycan",
        Role::Pacifist => "RoleInfoPacifist",
        Role::WiseElder => "RoleInfoWiseElder",
        Role::Oracle => "RoleInfoOracle",
        Role::Sandman => "RoleInfoSandman",
        Role::WolfMan => "RoleInfoWolfMan",
        Role::Thief => "RoleInfoThief",
        Role::Troublemaker => "RoleInfoTroublemaker",
        Role::Chemist => "RoleInfoChemist",
        Role::SnowWolf => "RoleInfoSnowWolf",
        Role::GraveDigger => "RoleInfoGraveDigger",
        Role::Augur => "RoleInfoAugur",
        Role::Arsonist => "RoleInfoArsonist",
        // Not in English.xml yet (the Spumpkin's other lines aren't either).
        Role::Spumpkin => "RoleInfoSpumpkin",
        Role::Chef => "RoleInfoChef",
        Role::Barkeep => "RoleInfoBarkeep",
        Role::Witch => "RoleInfoWitch",
    }
}

/// `GetRoleInfo` (Werewolf.cs:1643-1711): a role's description, with the
/// team a Mason, Cultist or wolf belongs to, and the Seer a Beholder
/// watches. `alive` is everyone alive now, the player included.
pub fn role_info(role: Role, alive: &[AlivePlayer], thief_full: bool) -> Message {
    let info = Message::new(role_info_key(role), vec![]);
    match role {
        Role::Beholder => info.followed_by(match holder(alive, Role::Seer) {
            Some(seer) => Message::new("BeholderSeer", vec![MessageArg::Player(seer)]),
            None => Message::new("NoSeer", vec![]),
        }),
        Role::Thief if thief_full => Message::new("RoleInfoThiefFull", vec![]),
        Role::Mason => {
            let masons = ids(alive, |p| p.role == Role::Mason);
            if masons.len() > 1 {
                info.followed_by(Message::new("MasonTeam", vec![names(masons)]))
            } else {
                info
            }
        }
        Role::Cultist => {
            let cult = ids(alive, |p| p.role == Role::Cultist);
            if cult.len() > 1 {
                info.followed_by(Message::new("CultistsList", vec![names(cult)]))
            } else {
                info
            }
        }
        Role::Wolf | Role::WolfCub | Role::AlphaWolf | Role::Lycan | Role::SnowWolf => {
            let pack = ids(alive, |p| eats(p.role) || p.role == Role::SnowWolf);
            if pack.len() < 2 {
                return info;
            }
            info.followed_by(pack_list(alive, pack))
        }
        _ => info,
    }
}

/// `WolfPackList`, or `WolfPackSnowList` naming the Snow Wolf apart.
fn pack_list(alive: &[AlivePlayer], pack: Vec<PlayerId>) -> Message {
    match holder(alive, Role::SnowWolf) {
        Some(snow) => {
            let eaters = pack.into_iter().filter(|&id| id != snow).collect();
            Message::new("WolfPackSnowList", vec![names(eaters), MessageArg::Player(snow)])
        }
        None => Message::new("WolfPackList", vec![names(pack)]),
    }
}

/// Why a role changed: the legacy `TransformationMethod`, with the player
/// each one names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// The wolves attacked the Cursed (Werewolf.cs:3301).
    BiteCursed,
    /// An Alpha Wolf's bite took hold (Werewolf.cs:2996).
    AlphaBitten,
    /// No wolf left (Werewolf.cs:4507).
    Traitor,
    /// The Snow Wolf is the last wolf (Werewolf.cs:4493-4495).
    SnowWolfAlone,
    /// The Seer died; `model` is the Seer who died, if known.
    ApprenticeSeer {
        model: Option<PlayerId>,
    },
    ConvertToCult,
    WildChild {
        model: PlayerId,
    },
    Doppelganger {
        model: PlayerId,
    },
    /// The Thief took `from`'s role.
    ThiefSteal {
        from: PlayerId,
    },
    /// The Thief's victim, told by its own note (`ThiefStoleYourRole...`).
    ThiefStolen,
    /// A Gunner, Hunter or Chemist who killed the Wise Elder; the group
    /// line says so.
    KillElder,
    /// The cult's last converts at the end of the game: nothing is said.
    AutoConvertToCult,
}

/// What `Transform` tells the changed player and the teams they left and
/// joined (Werewolf.cs:1998-2260). `alive` is everyone alive *after* the
/// change, `player` already holding `to`.
pub fn transform_tells(
    player: PlayerId,
    from: Role,
    to: Role,
    cause: Cause,
    alive: &[AlivePlayer],
    thief_full: bool,
) -> Vec<NarrationEvent> {
    let others = |keep: &dyn Fn(Role) -> bool| ids(alive, |p| p.id != player && keep(p.role));
    let me = MessageArg::Player(player);
    let mut out = vec![];
    // Old teammates hear that one of theirs is gone (Werewolf.cs:1998-2006).
    let left = match cause {
        Cause::AlphaBitten | Cause::ConvertToCult => from != to,
        _ => false,
    };
    if left && from == Role::Mason {
        out.extend(tell_all(
            &others(&|r| r == Role::Mason),
            Message::new("MasonConverted", vec![me.clone()]),
        ));
    }
    if left && from == Role::Cultist {
        out.extend(tell_all(
            &others(&|r| r == Role::Cultist),
            Message::new("CultistBitten", vec![me.clone()]),
        ));
    }
    let eaters = others(&|r| eats(r));
    let snow = others(&|r| r == Role::SnowWolf).first().copied();
    let beholder = holder(alive, Role::Beholder);
    match cause {
        Cause::BiteCursed | Cause::AlphaBitten => {
            let key = if cause == Cause::BiteCursed {
                "CursedBitten"
            } else {
                "BittenTurned"
            };
            let mut message = Message::new(key, vec![]);
            let pack: Vec<PlayerId> = eaters.iter().copied().chain(snow).collect();
            if !pack.is_empty() {
                message = message.followed_by(pack_list(alive, pack.clone()));
            }
            out.push(tell(player, message));
            if cause == Cause::BiteCursed {
                out.extend(tell_all(&pack, Message::new("CursedBittenToWolves", vec![me])));
            }
        }
        Cause::Traitor => out.push(tell(player, Message::new("TraitorTurnWolf", vec![]))),
        Cause::SnowWolfAlone => out.push(tell(player, Message::new("SnowWolfTurnWolf", vec![]))),
        Cause::ApprenticeSeer { model } => {
            let seer = model.map(MessageArg::Player).unwrap_or(MessageArg::Role(Role::Seer));
            out.push(tell(player, Message::new("ApprenticeNowSeer", vec![seer.clone()])));
            if let Some(b) = beholder {
                out.push(tell(b, Message::new("BeholderNewSeer", vec![me, seer])));
            }
        }
        Cause::ConvertToCult => {
            let cult = others(&|r| r == Role::Cultist);
            out.push(tell(player, Message::new("CultConvertYou", vec![])));
            out.push(tell(player, Message::new("CultTeam", vec![names(cult.clone())])));
            let everyone = ids(alive, |p| p.role == Role::Cultist);
            let join =
                Message::new("CultJoin", vec![me]).followed_by(Message::new("CultistsList", vec![names(everyone)]));
            out.extend(tell_all(&cult, join));
        }
        Cause::WildChild { model } => {
            out.extend(tell_all(
                &eaters.iter().copied().chain(snow).collect::<Vec<_>>(),
                Message::new("WildChildToWolves", vec![me]),
            ));
            let model = MessageArg::Player(model);
            out.push(tell(
                player,
                match snow {
                    Some(s) => Message::new(
                        "WildChildSnowTransform",
                        vec![model, names(eaters), MessageArg::Player(s)],
                    ),
                    None => Message::new("WildChildTransform", vec![model, names(eaters)]),
                },
            ));
        }
        Cause::Doppelganger { model } => {
            let model = MessageArg::Player(model);
            let wolfish = matches!(to, Role::Wolf | Role::AlphaWolf | Role::WolfCub | Role::Lycan);
            let teamed = wolfish || matches!(to, Role::Mason | Role::Cultist | Role::WildChild | Role::SnowWolf);
            if !teamed {
                out.push(tell(player, role_info(to, alive, thief_full)));
            }
            match to {
                Role::Mason => {
                    let masons = others(&|r| r == Role::Mason);
                    out.extend(tell_all(&masons, Message::new("DGToMason", vec![me])));
                    out.push(tell(
                        player,
                        Message::new("DGTransformToMason", vec![model, names(masons)]),
                    ));
                }
                Role::Seer => {
                    if let Some(b) = beholder {
                        out.push(tell(b, Message::new("BeholderNewSeer", vec![me, model])));
                    }
                }
                _ if wolfish => {
                    let pack: Vec<PlayerId> = eaters.iter().copied().chain(snow).collect();
                    out.extend(tell_all(&pack, Message::new("DGToWolf", vec![me])));
                    let (plain, with_snow) = match to {
                        Role::AlphaWolf => ("DGTransformToAlpha", "DGTransformToAlphaWithSnow"),
                        Role::WolfCub => ("DGTransformToWolfCub", "DGTransformToWolfCubWithSnow"),
                        Role::Lycan => ("DGTransformToLycan", "DGTransformToLycanWithSnow"),
                        _ => ("DGTransformToWolf", "DGTransformToWolfWithSnow"),
                    };
                    out.push(tell(
                        player,
                        match snow {
                            Some(s) => Message::new(with_snow, vec![model, names(eaters), MessageArg::Player(s)]),
                            None => Message::new(plain, vec![model, names(eaters)]),
                        },
                    ));
                }
                Role::SnowWolf => {
                    out.extend(tell_all(&eaters, Message::new("DGToSnowWolf", vec![me])));
                    out.push(tell(
                        player,
                        Message::new("DGTransformToSnowWolf", vec![model, names(eaters)]),
                    ));
                }
                Role::Cultist => {
                    let cult = others(&|r| r == Role::Cultist);
                    out.extend(tell_all(&cult, Message::new("DGToCult", vec![me])));
                    out.push(tell(
                        player,
                        Message::new("DGTransformToCult", vec![model, names(cult)]),
                    ));
                }
                _ => {}
            }
        }
        Cause::ThiefSteal { from: victim } => {
            out.push(tell(player, role_info(to, alive, thief_full)));
            let victim = MessageArg::Player(victim);
            let (team, key) = match to {
                Role::Mason => (others(&|r| r == Role::Mason), "ThiefToMason"),
                Role::Wolf | Role::AlphaWolf | Role::WolfCub | Role::Lycan => {
                    (eaters.iter().copied().chain(snow).collect(), "ThiefToWolf")
                }
                Role::SnowWolf => (eaters.clone(), "ThiefToSnowWolf"),
                Role::Cultist => (others(&|r| r == Role::Cultist), "ThiefToCult"),
                _ => (vec![], ""),
            };
            out.extend(tell_all(&team, Message::new(key, vec![victim.clone(), me.clone()])));
            if to == Role::Seer {
                if let Some(b) = beholder {
                    out.push(tell(b, Message::new("BeholderSeerStolen", vec![me, victim])));
                }
            }
        }
        Cause::ThiefStolen | Cause::KillElder | Cause::AutoConvertToCult => {}
    }
    out
}

/// One of each message this module can build, for the locale coverage
/// check.
pub fn samples() -> Vec<Message> {
    let p = PlayerId(1);
    let q = PlayerId(2);
    let s = PlayerId(3);
    let roster = |roles: &[Role]| -> Vec<AlivePlayer> {
        roles
            .iter()
            .enumerate()
            .map(|(i, &role)| AlivePlayer {
                id: PlayerId(i as u64 + 1),
                role,
            })
            .collect()
    };
    let mut out: Vec<Message> = vec![];
    let mut add = |events: Vec<NarrationEvent>| {
        for e in events {
            if let NarrationEvent::Tell { message, .. } = e {
                let mut m = Some(message);
                while let Some(next) = m {
                    m = next.then.as_deref().cloned();
                    out.push(Message { then: None, ..next });
                }
            }
        }
    };
    let packs = [
        roster(&[Role::Wolf, Role::Wolf, Role::Beholder, Role::Mason, Role::Cultist]),
        roster(&[Role::Wolf, Role::Wolf, Role::SnowWolf, Role::Beholder, Role::Seer]),
    ];
    for alive in &packs {
        for role in [Role::Wolf, Role::Beholder, Role::Mason, Role::Cultist, Role::Thief] {
            add(vec![
                tell(p, role_info(role, alive, false)),
                tell(p, role_info(role, alive, true)),
            ]);
        }
        let causes = [
            Cause::BiteCursed,
            Cause::AlphaBitten,
            Cause::Traitor,
            Cause::SnowWolfAlone,
            Cause::ApprenticeSeer { model: Some(q) },
            Cause::ConvertToCult,
            Cause::WildChild { model: q },
        ];
        for cause in causes {
            add(transform_tells(p, Role::Mason, Role::Wolf, cause, alive, false));
            add(transform_tells(p, Role::Cultist, Role::Wolf, cause, alive, false));
        }
        for to in [
            Role::Mason,
            Role::Seer,
            Role::Wolf,
            Role::AlphaWolf,
            Role::WolfCub,
            Role::Lycan,
            Role::SnowWolf,
            Role::Cultist,
        ] {
            add(transform_tells(
                s,
                Role::Doppelganger,
                to,
                Cause::Doppelganger { model: q },
                alive,
                false,
            ));
            add(transform_tells(
                s,
                Role::Thief,
                to,
                Cause::ThiefSteal { from: q },
                alive,
                false,
            ));
        }
    }
    for role in shared::ALL_ROLES {
        if !matches!(role, Role::Spumpkin | Role::Chef | Role::Barkeep | Role::Witch) {
            out.push(Message::new(role_info_key(*role), vec![]));
        }
    }
    out
}
