//! The check every command passes before it runs: who sent it, where, and
//! whether the command allows that (UpdateHandler.cs:327-440 and the
//! `[Command(...)]` attribute on each command).

/// What a command demands of its sender, mirroring the C# `Command`
/// attribute's flags one for one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommandPolicy {
    pub dev_only: bool,
    pub lang_admin_only: bool,
    pub global_admin_only: bool,
    pub group_admin_only: bool,
    pub in_group_only: bool,
    /// Refused in the support and translation chats, where games aren't
    /// allowed.
    pub blockable: bool,
    /// An admin posting anonymously as the group may use it.
    pub allow_anonymous_admins: bool,
}

const ANYONE: CommandPolicy = CommandPolicy {
    dev_only: false,
    lang_admin_only: false,
    global_admin_only: false,
    group_admin_only: false,
    in_group_only: false,
    blockable: false,
    allow_anonymous_admins: false,
};
const GROUP_PLAYER: CommandPolicy = CommandPolicy {
    blockable: true,
    in_group_only: true,
    ..ANYONE
};
const GROUP_ADMIN: CommandPolicy = CommandPolicy {
    group_admin_only: true,
    allow_anonymous_admins: true,
    ..GROUP_PLAYER
};
const GLOBAL_ADMIN: CommandPolicy = CommandPolicy {
    global_admin_only: true,
    ..ANYONE
};

/// The policy of every command this port implements, copied from its C#
/// attribute (GameCommands.cs, AdminCommands.cs, DevCommands.cs,
/// GeneralCommands.cs). `None` for a trigger the port doesn't handle.
pub fn policy_for(trigger: &str) -> Option<CommandPolicy> {
    let policy = match trigger.to_ascii_lowercase().as_str() {
        "join" | "flee" | "startgame" | "startchaos" | "players" | "extend" => GROUP_PLAYER,
        "forcestart" | "smite" => GROUP_ADMIN,
        "config" => CommandPolicy {
            group_admin_only: true,
            in_group_only: true,
            ..ANYONE
        },
        "killgame" => CommandPolicy {
            in_group_only: true,
            ..GLOBAL_ADMIN
        },
        "getban" | "permban" | "remban" | "restore" => GLOBAL_ADMIN,
        "uploadlang" | "validatelangs" => CommandPolicy {
            lang_admin_only: true,
            ..ANYONE
        },
        "getidles" => CommandPolicy {
            group_admin_only: true,
            allow_anonymous_admins: true,
            ..ANYONE
        },
        "start" | "help" | "ping" | "setlang" | "getlang" => ANYONE,
        _ => return None,
    };
    Some(policy)
}

/// A chat where games are blocked (UpdateHandler.cs:313). Each one has its
/// own hardcoded notice (UpdateHandler.cs:393-405).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockedChat {
    Support,
    PersianSupport,
    Translation,
}

impl BlockedChat {
    pub fn notice(self) -> &'static str {
        match self {
            BlockedChat::Support => "No games in support chat!\nIf you want to play, find a group in the /grouplist.",
            BlockedChat::PersianSupport => "اینجا گروه پشتیبانیه نه بازی، لطفا دکمه استارت رو نزنید.",
            BlockedChat::Translation => "No games in translation group!",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chat {
    Private,
    Group { blocked: Option<BlockedChat> },
}

/// Who a message claims to come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sender {
    User,
    /// A group admin posting anonymously, as the group itself.
    AnonymousGroupAdmin,
    /// Any other sender chat, e.g. a linked channel posting into the group.
    Channel,
}

/// Everything the gate needs to know about one incoming command. The
/// caller works out the roles (admin lists, the dev list, the ban list)
/// from whatever storage and Telegram lookups it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Invocation {
    pub sender: Sender,
    pub chat: Chat,
    pub sender_is_banned: bool,
    pub sender_is_dev: bool,
    pub sender_is_global_admin: bool,
    pub sender_is_lang_admin: bool,
    pub sender_is_group_admin: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    Run,
    /// Drop the command without a word: the sender is banned.
    Ignore,
    Refuse(Refusal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    ExitAnonymousMode,
    BlockedChat(BlockedChat),
    /// Dev-only command, non-dev sender (`NotPara`).
    NotPara,
    NotGlobalAdmin,
    GroupCommandOnly,
    GroupAdminOnly,
}

impl Refusal {
    /// The English.xml key the C# sent, or `None` for the blocked-chat
    /// notices, which were hardcoded strings.
    pub fn locale_key(self) -> Option<&'static str> {
        match self {
            Refusal::ExitAnonymousMode => Some("ExitAnonymousMode"),
            Refusal::BlockedChat(_) => None,
            Refusal::NotPara => Some("NotPara"),
            Refusal::NotGlobalAdmin => Some("NotGlobalAdmin"),
            Refusal::GroupCommandOnly => Some("GroupCommandOnly"),
            Refusal::GroupAdminOnly => Some("GroupAdminOnly"),
        }
    }

    pub const ALL_LOCALE_KEYS: [&'static str; 5] = [
        "ExitAnonymousMode",
        "NotPara",
        "NotGlobalAdmin",
        "GroupCommandOnly",
        "GroupAdminOnly",
    ];
}

/// Decides whether a command runs, in the same order the C# checked
/// (UpdateHandler.cs:330-440), so a sender failing several checks gets
/// the same refusal they always did.
///
/// Not ported: the per-user flood counter (`AddCount`, UpdateHandler.cs:
/// 391) is rate limiting, which belongs to the transport layer, and the
/// one silent refusal for the `/user` command, which isn't ported.
pub fn check(policy: &CommandPolicy, call: &Invocation) -> GateDecision {
    if call.sender_is_banned {
        return GateDecision::Ignore;
    }

    let anonymous_refused = match call.sender {
        Sender::User => false,
        Sender::AnonymousGroupAdmin => !policy.allow_anonymous_admins,
        Sender::Channel => true,
    };
    if anonymous_refused {
        return GateDecision::Refuse(Refusal::ExitAnonymousMode);
    }

    if policy.blockable {
        if let Chat::Group { blocked: Some(chat) } = call.chat {
            return GateDecision::Refuse(Refusal::BlockedChat(chat));
        }
    }
    if policy.dev_only && !call.sender_is_dev {
        return GateDecision::Refuse(Refusal::NotPara);
    }
    if policy.lang_admin_only && !call.sender_is_lang_admin && !call.sender_is_global_admin {
        return GateDecision::Refuse(Refusal::NotGlobalAdmin);
    }
    if policy.global_admin_only && !call.sender_is_global_admin {
        return GateDecision::Refuse(Refusal::NotGlobalAdmin);
    }
    if policy.in_group_only && call.chat == Chat::Private {
        return GateDecision::Refuse(Refusal::GroupCommandOnly);
    }
    let is_anonymous_admin = call.sender == Sender::AnonymousGroupAdmin;
    if policy.group_admin_only
        && !is_anonymous_admin
        && !call.sender_is_group_admin
        && !call.sender_is_dev
        && !call.sender_is_global_admin
    {
        return GateDecision::Refuse(Refusal::GroupAdminOnly);
    }
    GateDecision::Run
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUP: Chat = Chat::Group { blocked: None };

    fn player() -> Invocation {
        Invocation {
            sender: Sender::User,
            chat: GROUP,
            sender_is_banned: false,
            sender_is_dev: false,
            sender_is_global_admin: false,
            sender_is_lang_admin: false,
            sender_is_group_admin: false,
        }
    }

    fn smite() -> CommandPolicy {
        policy_for("smite").unwrap()
    }

    fn getban() -> CommandPolicy {
        policy_for("getban").unwrap()
    }

    #[test]
    fn a_banned_user_is_ignored_even_for_open_commands() {
        let call = Invocation {
            sender_is_banned: true,
            ..player()
        };
        assert_eq!(check(&policy_for("ping").unwrap(), &call), GateDecision::Ignore);
        let admin = Invocation {
            sender_is_global_admin: true,
            ..call
        };
        assert_eq!(check(&getban(), &admin), GateDecision::Ignore);
    }

    #[test]
    fn group_admin_commands_accept_every_kind_of_admin() {
        let group_admin = Invocation {
            sender_is_group_admin: true,
            ..player()
        };
        let global_admin = Invocation {
            sender_is_global_admin: true,
            ..player()
        };
        let dev = Invocation {
            sender_is_dev: true,
            ..player()
        };
        let anonymous = Invocation {
            sender: Sender::AnonymousGroupAdmin,
            ..player()
        };
        for call in [group_admin, global_admin, dev, anonymous] {
            assert_eq!(check(&smite(), &call), GateDecision::Run, "{call:?}");
        }
    }

    #[test]
    fn group_admin_commands_refuse_ordinary_players() {
        assert_eq!(
            check(&smite(), &player()),
            GateDecision::Refuse(Refusal::GroupAdminOnly)
        );
    }

    #[test]
    fn group_only_commands_are_refused_in_private() {
        let call = Invocation {
            chat: Chat::Private,
            sender_is_group_admin: true,
            ..player()
        };
        assert_eq!(check(&smite(), &call), GateDecision::Refuse(Refusal::GroupCommandOnly));
    }

    #[test]
    fn global_admin_commands_refuse_group_admins() {
        let call = Invocation {
            sender_is_group_admin: true,
            ..player()
        };
        assert_eq!(check(&getban(), &call), GateDecision::Refuse(Refusal::NotGlobalAdmin));
        let admin = Invocation {
            sender_is_global_admin: true,
            chat: Chat::Private,
            ..player()
        };
        assert_eq!(check(&getban(), &admin), GateDecision::Run);
    }

    #[test]
    fn anonymous_admins_only_where_allowed() {
        let call = Invocation {
            sender: Sender::AnonymousGroupAdmin,
            ..player()
        };
        assert_eq!(
            check(&getban(), &call),
            GateDecision::Refuse(Refusal::ExitAnonymousMode)
        );
    }

    #[test]
    fn channels_are_always_refused() {
        let call = Invocation {
            sender: Sender::Channel,
            sender_is_global_admin: true,
            ..player()
        };
        assert_eq!(check(&smite(), &call), GateDecision::Refuse(Refusal::ExitAnonymousMode));
        assert_eq!(
            check(&policy_for("ping").unwrap(), &call),
            GateDecision::Refuse(Refusal::ExitAnonymousMode)
        );
    }

    #[test]
    fn blockable_commands_are_refused_in_support_chats() {
        let call = Invocation {
            chat: Chat::Group {
                blocked: Some(BlockedChat::Support),
            },
            sender_is_group_admin: true,
            ..player()
        };
        assert_eq!(
            check(&smite(), &call),
            GateDecision::Refuse(Refusal::BlockedChat(BlockedChat::Support))
        );
        // Not blockable: global admin commands still work there.
        let admin = Invocation {
            sender_is_global_admin: true,
            ..call
        };
        assert_eq!(check(&getban(), &admin), GateDecision::Run);
    }

    #[test]
    fn the_anonymous_check_comes_before_the_blocked_chat_check() {
        let call = Invocation {
            sender: Sender::Channel,
            chat: Chat::Group {
                blocked: Some(BlockedChat::Translation),
            },
            ..player()
        };
        assert_eq!(check(&smite(), &call), GateDecision::Refuse(Refusal::ExitAnonymousMode));
    }

    #[test]
    fn dev_only_refuses_with_not_para() {
        let policy = CommandPolicy {
            dev_only: true,
            ..Default::default()
        };
        let admin = Invocation {
            sender_is_global_admin: true,
            ..player()
        };
        assert_eq!(check(&policy, &admin), GateDecision::Refuse(Refusal::NotPara));
    }

    #[test]
    fn lang_admin_commands_accept_global_admins() {
        let policy = CommandPolicy {
            lang_admin_only: true,
            ..Default::default()
        };
        assert_eq!(check(&policy, &player()), GateDecision::Refuse(Refusal::NotGlobalAdmin));
        let lang = Invocation {
            sender_is_lang_admin: true,
            ..player()
        };
        let global = Invocation {
            sender_is_global_admin: true,
            ..player()
        };
        assert_eq!(check(&policy, &lang), GateDecision::Run);
        assert_eq!(check(&policy, &global), GateDecision::Run);
    }

    #[test]
    fn policies_match_the_csharp_attributes() {
        assert_eq!(policy_for("SMITE"), policy_for("smite"));
        let join = policy_for("join").unwrap();
        assert!(join.blockable && join.in_group_only && !join.group_admin_only);
        let killgame = policy_for("killgame").unwrap();
        assert!(killgame.global_admin_only && killgame.in_group_only && !killgame.blockable);
        let config = policy_for("config").unwrap();
        assert!(config.group_admin_only && !config.allow_anonymous_admins && !config.blockable);
        let uploadlang = policy_for("uploadlang").unwrap();
        assert!(uploadlang.lang_admin_only && !uploadlang.global_admin_only && !uploadlang.in_group_only);
        assert_eq!(policy_for("asplode"), None);
    }

    #[test]
    fn every_refusal_but_blocked_chats_has_a_locale_key() {
        let keys: Vec<_> = [
            Refusal::ExitAnonymousMode,
            Refusal::NotPara,
            Refusal::NotGlobalAdmin,
            Refusal::GroupCommandOnly,
            Refusal::GroupAdminOnly,
        ]
        .iter()
        .map(|r| r.locale_key().unwrap())
        .collect();
        assert_eq!(keys, Refusal::ALL_LOCALE_KEYS);
        assert_eq!(Refusal::BlockedChat(BlockedChat::Support).locale_key(), None);
    }
}
