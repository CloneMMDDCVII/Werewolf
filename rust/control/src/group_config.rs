//! Per-group settings and the `/config` menu (`AdminCommands.cs:66-100`,
//! the config half of `UpdateHandler.cs::HandleCallback` 1039-1985, and
//! the menu builders at `UpdateHandler.cs:2169-2300`).
//!
//! Two pieces:
//!
//! - [`GroupSettings`]: the typed settings one group carries. The legacy
//!   `Group` row spread these over nullable columns plus two `i64`
//!   bitflag columns (`Flags`/`GroupConfig.cs`, `RoleFlags`/`IRole`).
//!   Like `balancing`'s `disabled_roles`, the bitflags are a storage
//!   detail, not a rule, so toggles are a [`Toggle`] set and disabled
//!   roles a plain set of [`Role`]s. Decoding legacy rows belongs with the
//!   database migration.
//! - The menu: [`config_command`] answers `/config`, [`handle_callback`]
//!   answers a button press. Both are pure; [`run_config_command`] and
//!   [`run_callback`] wrap them with loading and saving through
//!   `db::Store`, where settings live as one JSON document per group. Both return [`Screen`]s (a locale key, its
//!   arguments, and typed buttons) rather than Telegram markup, so the
//!   engine-never-knows-about-Telegram rule holds here too. A presenter
//!   renders a `Screen`; [`ConfigAction::encode`]/[`ConfigAction::decode`]
//!   give it callback data that fits Telegram's 64-byte limit.
//!
//! Deliberately not here (see `COMMANDS.md`): the language picker (ported
//! with the per-user `/setlang`, which shares `SelectLanguage`), and the
//! five `Allow{Tanner,Fool,Cult,Thief,Arsonist}` flags - tagged
//! `RoleConfig` in `GroupConfig.cs`, but the RoleConfig submenu shows the
//! per-role toggles instead (`UpdateHandler.cs:2199`), so no admin could
//! reach them, and the node never reads them for balancing.

use crate::lobby::{DEFAULT_MAX_EXTEND_SECONDS, DEFAULT_MAX_PLAYERS};
use db::{DbResult, Store};
use game_engine::balancing::balance;
use game_engine::rng::RngSource;
use serde::{Deserialize, Serialize};
use shared::{GameMode, Role, ALL_ROLES};
use std::collections::{BTreeSet, HashSet};

/// `Werewolf Control/Helpers/Settings.cs:112-114`.
pub const DEFAULT_DAY_SECONDS: u32 = 60;
pub const DEFAULT_NIGHT_SECONDS: u32 = 90;
pub const DEFAULT_LYNCH_SECONDS: u32 = 90;

/// `GameBalancing.cs:137`: `TryBalance` checks every player count from
/// five up to the group's max.
const VALIDATION_MIN_PLAYERS: usize = 5;

/// `Group.Mode`: which game modes a group allows. `Player` lets whoever
/// starts the game pick (`/startgame` vs `/startchaos`); the other two
/// override that choice (`Werewolf.cs:204`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModeSetting {
    Normal,
    Chaos,
    Player,
}

impl ModeSetting {
    pub const ALL: [ModeSetting; 3] = [ModeSetting::Normal, ModeSetting::Chaos, ModeSetting::Player];

    /// The raw string the legacy row stores, and what `GameModeQ`/
    /// `GameModeA` print (`UpdateHandler.cs:1607,1616`).
    pub fn legacy_name(self) -> &'static str {
        match self {
            ModeSetting::Normal => "Normal",
            ModeSetting::Chaos => "Chaos",
            ModeSetting::Player => "Player",
        }
    }

    fn button_key(self) -> &'static str {
        match self {
            ModeSetting::Normal => "NormalOnly",
            ModeSetting::Chaos => "ChaosOnly",
            ModeSetting::Player => "PlayerChoice",
        }
    }

    /// `Werewolf.cs:204`.
    pub fn resolve(self, requested: GameMode) -> GameMode {
        match self {
            ModeSetting::Normal => GameMode::Normal,
            ModeSetting::Chaos => GameMode::Chaos,
            ModeSetting::Player => requested,
        }
    }
}

/// `Group.ShowRolesEnd`: which roles the end-of-game summary reveals
/// (`Werewolf.cs:4906`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EndRoles {
    None,
    Living,
    All,
}

impl EndRoles {
    pub const ALL: [EndRoles; 3] = [EndRoles::None, EndRoles::Living, EndRoles::All];

    pub fn legacy_name(self) -> &'static str {
        match self {
            EndRoles::None => "None",
            EndRoles::Living => "Living",
            EndRoles::All => "All",
        }
    }

    fn button_key(self) -> &'static str {
        match self {
            EndRoles::None => "ShowNone",
            EndRoles::Living => "ShowLiving",
            EndRoles::All => "ShowAll",
        }
    }
}

/// How a yes/no setting words its two answers (`GroupConfig.cs`'s
/// `SettingQuestion`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wording {
    AllowDisallow,
    YesNo,
    ShowHide,
}

impl Wording {
    fn keys(self) -> (&'static str, &'static str) {
        match self {
            Wording::AllowDisallow => ("Allow", "Disallow"),
            Wording::YesNo => ("Yes", "No"),
            Wording::ShowHide => ("Show", "Hide"),
        }
    }
}

/// The editable yes/no settings (`GroupConfig.cs`), in the legacy enum
/// order, which is also the order they appear in their submenu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Toggle {
    EnableSecretLynch,
    RandomMode,
    AllowExtend,
    ShowRolesDeath,
    AllowFlee,
    ShowIDs,
    AllowNSFW,
    ThiefFull,
    SecretLynchShowVotes,
    SecretLynchShowVoters,
    ShufflePlayerList,
    BurningOverkill,
}

impl Toggle {
    pub const ALL: [Toggle; 12] = [
        Toggle::EnableSecretLynch,
        Toggle::RandomMode,
        Toggle::AllowExtend,
        Toggle::ShowRolesDeath,
        Toggle::AllowFlee,
        Toggle::ShowIDs,
        Toggle::AllowNSFW,
        Toggle::ThiefFull,
        Toggle::SecretLynchShowVotes,
        Toggle::SecretLynchShowVoters,
        Toggle::ShufflePlayerList,
        Toggle::BurningOverkill,
    ];

    /// The flag's name, which is also its button's locale key; `{name}Q`
    /// and `{name}A` are the question and confirmation keys.
    pub fn name(self) -> &'static str {
        match self {
            Toggle::EnableSecretLynch => "EnableSecretLynch",
            Toggle::RandomMode => "RandomMode",
            Toggle::AllowExtend => "AllowExtend",
            Toggle::ShowRolesDeath => "ShowRolesDeath",
            Toggle::AllowFlee => "AllowFlee",
            Toggle::ShowIDs => "ShowIDs",
            Toggle::AllowNSFW => "AllowNSFW",
            Toggle::ThiefFull => "ThiefFull",
            Toggle::SecretLynchShowVotes => "SecretLynchShowVotes",
            Toggle::SecretLynchShowVoters => "SecretLynchShowVoters",
            Toggle::ShufflePlayerList => "ShufflePlayerList",
            Toggle::BurningOverkill => "BurningOverkill",
        }
    }

    fn question_key(self) -> String {
        format!("{}Q", self.name())
    }

    fn answer_key(self) -> String {
        format!("{}A", self.name())
    }

    /// `GroupConfig.cs`'s `[DefaultValue]`.
    pub fn default_value(self) -> bool {
        matches!(self, Toggle::ShowRolesDeath | Toggle::AllowFlee)
    }

    pub fn wording(self) -> Wording {
        match self {
            Toggle::AllowExtend | Toggle::AllowFlee | Toggle::AllowNSFW | Toggle::BurningOverkill => {
                Wording::AllowDisallow
            }
            Toggle::EnableSecretLynch | Toggle::RandomMode | Toggle::ThiefFull | Toggle::ShufflePlayerList => {
                Wording::YesNo
            }
            Toggle::ShowRolesDeath | Toggle::ShowIDs | Toggle::SecretLynchShowVotes | Toggle::SecretLynchShowVoters => {
                Wording::ShowHide
            }
        }
    }

    pub fn submenu(self) -> Submenu {
        match self {
            Toggle::EnableSecretLynch
            | Toggle::RandomMode
            | Toggle::ShowRolesDeath
            | Toggle::ThiefFull
            | Toggle::SecretLynchShowVotes
            | Toggle::SecretLynchShowVoters => Submenu::Mechanics,
            Toggle::AllowExtend
            | Toggle::AllowFlee
            | Toggle::ShowIDs
            | Toggle::AllowNSFW
            | Toggle::ShufflePlayerList
            | Toggle::BurningOverkill => Submenu::GroupSettings,
        }
    }
}

/// One group's settings. `Default` applies the legacy defaults
/// (`Helpers.cs::MakeDefaultGroup`, 210-235); a stored document missing
/// a field reads back with that field's default. The group's name lives
/// on its `db` row, not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GroupSettings {
    pub mode: ModeSetting,
    pub max_players: u32,
    /// Stored as the legacy `DayTime` column: the menu shows this plus 60
    /// (`UpdateHandler.cs:1658-1665`).
    pub day_seconds: u32,
    pub night_seconds: u32,
    pub lynch_seconds: u32,
    pub max_extend_seconds: u32,
    pub end_roles: EndRoles,
    enabled: BTreeSet<Toggle>,
    disabled_roles: HashSet<Role>,
    /// `IRole.VALID`: set once the current disabled-role set has passed
    /// validation (or by "enable all"). Disabled roles only reach
    /// balancing while this is set (`GameBalancing.cs:37`).
    roles_validated: bool,
}

impl Default for GroupSettings {
    fn default() -> Self {
        GroupSettings {
            mode: ModeSetting::Player,
            max_players: DEFAULT_MAX_PLAYERS as u32,
            day_seconds: DEFAULT_DAY_SECONDS,
            night_seconds: DEFAULT_NIGHT_SECONDS,
            lynch_seconds: DEFAULT_LYNCH_SECONDS,
            max_extend_seconds: DEFAULT_MAX_EXTEND_SECONDS as u32,
            end_roles: EndRoles::All,
            enabled: Toggle::ALL.into_iter().filter(|t| t.default_value()).collect(),
            disabled_roles: HashSet::new(),
            roles_validated: false,
        }
    }
}

impl GroupSettings {
    pub fn is_on(&self, toggle: Toggle) -> bool {
        self.enabled.contains(&toggle)
    }

    pub fn set(&mut self, toggle: Toggle, on: bool) {
        if on {
            self.enabled.insert(toggle);
        } else {
            self.enabled.remove(&toggle);
        }
    }

    pub fn is_role_disabled(&self, role: Role) -> bool {
        self.disabled_roles.contains(&role)
    }

    pub fn roles_validated(&self) -> bool {
        self.roles_validated
    }

    /// The roles balancing should leave out: the admin's disabled set once
    /// it has been validated, and nothing before then (`GameBalancing.cs:37`).
    pub fn disabled_roles_for_balancing(&self) -> Vec<Role> {
        if !self.roles_validated {
            return vec![];
        }
        ALL_ROLES.iter().copied().filter(|r| self.disabled_roles.contains(r)).collect()
    }

    /// `UpdateHandler.cs:1851-1884` (`togglerole`): flips one role and
    /// drops validation. Roles that can't be disabled are ignored.
    pub fn toggle_role(&mut self, role: Role) {
        if !is_configurable_role(role) {
            return;
        }
        if !self.disabled_roles.remove(&role) {
            self.disabled_roles.insert(role);
        }
        self.roles_validated = false;
    }

    /// `togglerole|enableall`: everything on, and valid by definition.
    pub fn enable_all_roles(&mut self) {
        self.disabled_roles.clear();
        self.roles_validated = true;
    }

    /// `togglerole|disableall`: everything that can be disabled is, and
    /// validation is cleared.
    pub fn disable_all_roles(&mut self) {
        self.disabled_roles = configurable_roles().collect();
        self.roles_validated = false;
    }

    /// `UpdateHandler.cs:1886-1910` and `GameBalancing.cs::TryBalance`
    /// (135-150): the disabled set is valid when a normal game balances
    /// for every player count from five to `max_players`, checked with
    /// burning overkill on as the source does.
    pub fn validate_roles(&mut self, rng: &mut dyn RngSource) -> bool {
        let disabled: Vec<Role> = ALL_ROLES.iter().copied().filter(|r| self.disabled_roles.contains(r)).collect();
        let max = (self.max_players as usize).max(VALIDATION_MIN_PLAYERS);
        self.roles_validated =
            (VALIDATION_MIN_PLAYERS..=max).all(|count| balance(count, false, &disabled, true, rng).is_ok());
        self.roles_validated
    }

    /// `UpdateHandler.cs:1560-1574`: raising the cap can make a validated
    /// role set unbalanceable at the new sizes, so it clears validation;
    /// lowering it can't, so it doesn't.
    pub fn set_max_players(&mut self, max_players: u32) {
        if max_players > self.max_players {
            self.roles_validated = false;
        }
        self.max_players = max_players;
    }

    /// `Werewolf.cs:835`: whether a player may flee once the game is
    /// running. Fleeing during joining is always allowed. A refusal is
    /// reported with [`FLEE_DISABLED_KEY`].
    pub fn flee_allowed_while_running(&self) -> bool {
        self.is_on(Toggle::AllowFlee)
    }
}

/// `Werewolf.cs:837`.
pub const FLEE_DISABLED_KEY: &str = "FleeDisabled";

/// Every role an admin can turn off, in legacy menu order: all legacy
/// roles except Villager, Wolf and Spumpkin (`[Role(..., false)]`).
pub fn configurable_roles() -> impl Iterator<Item = Role> {
    ALL_ROLES.iter().copied().filter(|&r| is_configurable_role(r))
}

fn is_configurable_role(role: Role) -> bool {
    role.emoji().is_some() && !matches!(role, Role::Villager | Role::Wolf | Role::Spumpkin)
}

// ---------- Menu model ----------

/// The four config groups, in main-menu order (`ConfigGroup` enum order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Submenu {
    Timers,
    RoleConfig,
    Mechanics,
    GroupSettings,
}

impl Submenu {
    pub const ALL: [Submenu; 4] = [Submenu::Timers, Submenu::RoleConfig, Submenu::Mechanics, Submenu::GroupSettings];

    pub fn key(self) -> &'static str {
        match self {
            Submenu::Timers => "Timers",
            Submenu::RoleConfig => "RoleConfig",
            Submenu::Mechanics => "Mechanics",
            Submenu::GroupSettings => "GroupSettings",
        }
    }
}

/// A setting whose own screen asks a question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigOption {
    Mode,
    MaxPlayer,
    EndRoles,
    MaxExtend,
    DayTimer,
    LynchTimer,
    NightTimer,
    Toggle(Toggle),
}

impl ConfigOption {
    /// The option's button key in its submenu.
    pub fn key(self) -> &'static str {
        match self {
            ConfigOption::Mode => "Mode",
            ConfigOption::MaxPlayer => "MaxPlayer",
            ConfigOption::EndRoles => "EndRoles",
            ConfigOption::MaxExtend => "MaxExtend",
            ConfigOption::DayTimer => "DayTimer",
            ConfigOption::LynchTimer => "LynchTimer",
            ConfigOption::NightTimer => "NightTimer",
            ConfigOption::Toggle(t) => t.name(),
        }
    }

    pub fn submenu(self) -> Submenu {
        match self {
            ConfigOption::Mode | ConfigOption::MaxPlayer => Submenu::GroupSettings,
            ConfigOption::EndRoles => Submenu::Mechanics,
            ConfigOption::MaxExtend | ConfigOption::DayTimer | ConfigOption::LynchTimer | ConfigOption::NightTimer => {
                Submenu::Timers
            }
            ConfigOption::Toggle(t) => t.submenu(),
        }
    }
}

/// `ConfigGroupAttribute.hardcodedConfigOptions` followed by the toggles
/// tagged with that group, the order `GetConfigSubmenu` lists them in.
/// The legacy `Lang` button heads GroupSettings; it returns with the
/// language picker.
pub fn submenu_options(submenu: Submenu) -> Vec<ConfigOption> {
    let fixed: &[ConfigOption] = match submenu {
        Submenu::Timers => &[
            ConfigOption::MaxExtend,
            ConfigOption::DayTimer,
            ConfigOption::LynchTimer,
            ConfigOption::NightTimer,
        ],
        Submenu::Mechanics => &[ConfigOption::EndRoles],
        Submenu::GroupSettings => &[ConfigOption::Mode, ConfigOption::MaxPlayer],
        Submenu::RoleConfig => &[],
    };
    fixed
        .iter()
        .copied()
        .chain(Toggle::ALL.into_iter().filter(|t| t.submenu() == submenu).map(ConfigOption::Toggle))
        .collect()
}

const MAX_PLAYER_CHOICES: [u32; 7] = [5, 10, 15, 20, 25, 30, 35];
const MAX_EXTEND_CHOICES: [u32; 5] = [60, 120, 180, 240, 300];
const NIGHT_LYNCH_CHOICES: [u32; 4] = [30, 60, 90, 120];
/// Stored values; the buttons read 60 more (`UpdateHandler.cs:1658-1661`).
const DAY_CHOICES: [u32; 4] = [30, 60, 90, 120];
const DAY_DISPLAY_OFFSET: u32 = 60;

/// Everything a config button can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Callback {
    OpenSubmenu(Submenu),
    BackToMain,
    Done,
    OpenOption(ConfigOption),
    Cancel(ConfigOption),
    SetMode(ModeSetting),
    SetMaxPlayers(u32),
    SetEndRoles(EndRoles),
    SetMaxExtend(u32),
    SetDayTimer(u32),
    SetNightTimer(u32),
    SetLynchTimer(u32),
    SetToggle(Toggle, bool),
    ToggleRole(Role),
    EnableAllRoles,
    DisableAllRoles,
    ValidateRoles,
    /// The inert "Valid" button shown once roles are validated (the
    /// legacy `"dummmy"` callback).
    Nothing,
}

/// A callback addressed to one group, as carried in button data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigAction {
    pub group_id: i64,
    pub callback: Callback,
}

/// A piece of text a presenter resolves in the viewer's language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Text {
    Locale(String),
    Raw(String),
}

impl Text {
    fn key(key: &str) -> Self {
        Text::Locale(key.to_string())
    }
}

/// A locale string and the arguments substituted into its `{0}`, `{1}`...
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub key: String,
    pub args: Vec<Text>,
}

impl Message {
    fn new(key: &str) -> Self {
        Message { key: key.to_string(), args: vec![] }
    }

    fn with(key: &str, args: Vec<Text>) -> Self {
        Message { key: key.to_string(), args }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ButtonAction {
    Callback(ConfigAction),
    /// A link that opens a private chat with the bot (`RequestPM`'s
    /// "Start Me" button).
    StartBotLink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub label: Text,
    pub action: ButtonAction,
}

/// What a presenter shows: a message and its keyboard, one inner `Vec`
/// per row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    pub message: Message,
    pub rows: Vec<Vec<Button>>,
}

/// A short notice on the button press itself (Telegram's callback answer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub message: Message,
    /// Shown as a dialog rather than a passing toast.
    pub alert: bool,
}

/// The answer to `/config`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutcome {
    /// Send `menu` to the admin privately. If that fails because they
    /// never started the bot, send `fallback` to the group instead
    /// (`AdminCommands.cs:96-99`).
    SendPrivately { menu: Screen, fallback: Screen },
    /// Reply in the chat the command came from with this locale string.
    Refuse(Message),
}

/// The answer to a button press.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallbackOutcome {
    /// Replace the pressed message with `screen`, after showing `notice`.
    Show { screen: Screen, notice: Option<Notice> },
    /// Answer the press with a notice and leave the message alone.
    Notify(Notice),
    /// Nothing to do (unknown group, or the inert "Valid" button).
    Ignore,
}

/// Who sent a command or pressed a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Actor {
    /// A group admin, a global admin, or a developer: the three roles the
    /// legacy checks accept (`UpdateHandler.cs:1031`).
    pub can_configure: bool,
}

/// Where `/config` was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandChat {
    pub chat_id: i64,
    pub title: String,
    pub is_group: bool,
}

/// `/config` (`AdminCommands.cs:66-100`, gated by `UpdateHandler.cs:428-440`).
/// On `SendPrivately` the caller records the group and its current name
/// (the legacy command adds the group row and refreshes `Name`); see
/// [`run_config_command`].
pub fn config_command(chat: &CommandChat, actor: Actor) -> CommandOutcome {
    if !chat.is_group {
        return CommandOutcome::Refuse(Message::new("GroupCommandOnly"));
    }
    if !actor.can_configure {
        return CommandOutcome::Refuse(Message::new("GroupAdminOnly"));
    }
    CommandOutcome::SendPrivately { menu: main_menu(chat.chat_id), fallback: start_me_prompt() }
}

/// [`config_command`], recording the group in `store` when it succeeds.
pub async fn run_config_command(store: &Store, chat: &CommandChat, actor: Actor, now: i64) -> DbResult<CommandOutcome> {
    let outcome = config_command(chat, actor);
    if matches!(outcome, CommandOutcome::SendPrivately { .. }) {
        store.groups().upsert_name(chat.chat_id, &chat.title, now).await?;
    }
    Ok(outcome)
}

/// A group's settings, or `None` if the bot has never recorded the group.
pub async fn load_settings(store: &Store, group_id: i64) -> DbResult<Option<GroupSettings>> {
    if store.groups().get(group_id).await?.is_none() {
        return Ok(None);
    }
    store.groups().settings::<GroupSettings>(group_id).await.map(Some)
}

/// [`handle_callback`] against the settings in `store`, saving them if
/// the press changed anything.
pub async fn run_callback(
    store: &Store,
    action: ConfigAction,
    actor: Actor,
    rng: &mut (dyn RngSource + Send),
    now: i64,
) -> DbResult<CallbackOutcome> {
    let Some(record) = store.groups().get(action.group_id).await? else {
        return Ok(handle_callback(None, action, actor, rng));
    };
    let mut settings = load_settings(store, action.group_id).await?.unwrap_or_default();
    let before = settings.clone();
    let outcome = handle_callback(Some(&mut settings), action, actor, rng);
    if settings != before {
        store.groups().put_settings(action.group_id, &record.name, &settings, now).await?;
    }
    Ok(outcome)
}

/// `Helpers.cs::RequestPM`.
pub fn start_me_prompt() -> Screen {
    Screen {
        message: Message::new("StartMe"),
        rows: vec![vec![Button { label: Text::Raw("Start Me".to_string()), action: ButtonAction::StartBotLink }]],
    }
}

/// A button press (`UpdateHandler.cs:1026-1985`, config commands only).
/// `settings` is `None` for a group the bot has no record of, which the
/// legacy handler ignores.
pub fn handle_callback(
    settings: Option<&mut GroupSettings>,
    action: ConfigAction,
    actor: Actor,
    rng: &mut dyn RngSource,
) -> CallbackOutcome {
    let group_id = action.group_id;

    // `done` is the one config callback that needs neither a group nor an
    // admin (`UpdateHandler.cs:1026`).
    if action.callback == Callback::Done {
        return show(Screen { message: Message::new("ThankYou"), rows: vec![] }, None);
    }

    let Some(settings) = settings else {
        return CallbackOutcome::Ignore;
    };
    if !actor.can_configure {
        return CallbackOutcome::Notify(Notice { message: Message::new("GroupAdminOnly"), alert: true });
    }

    match action.callback {
        Callback::Done => unreachable!("handled above"),
        Callback::Nothing => CallbackOutcome::Ignore,
        Callback::BackToMain => show(main_menu(group_id), None),
        Callback::OpenSubmenu(submenu) => show(submenu_screen(group_id, settings, submenu), None),
        Callback::Cancel(option) => show(submenu_screen(group_id, settings, option.submenu()), None),
        Callback::OpenOption(option) => show(option_screen(group_id, settings, option), None),

        Callback::SetMode(mode) => {
            settings.mode = mode;
            confirm(group_id, settings, ConfigOption::Mode, "GameModeA", Text::Raw(mode.legacy_name().into()))
        }
        Callback::SetMaxPlayers(n) => {
            settings.set_max_players(n);
            confirm(group_id, settings, ConfigOption::MaxPlayer, "MaxPlayersA", raw_number(n))
        }
        Callback::SetEndRoles(end) => {
            settings.end_roles = end;
            confirm(group_id, settings, ConfigOption::EndRoles, "ShowRolesEndA", Text::Raw(end.legacy_name().into()))
        }
        Callback::SetMaxExtend(n) => {
            settings.max_extend_seconds = n;
            confirm(group_id, settings, ConfigOption::MaxExtend, "MaxExtendA", raw_number(n))
        }
        Callback::SetDayTimer(n) => {
            settings.day_seconds = n;
            confirm(group_id, settings, ConfigOption::DayTimer, "SetDayTimeA", raw_number(n + DAY_DISPLAY_OFFSET))
        }
        Callback::SetNightTimer(n) => {
            settings.night_seconds = n;
            confirm(group_id, settings, ConfigOption::NightTimer, "SetNightTimeA", raw_number(n))
        }
        Callback::SetLynchTimer(n) => {
            settings.lynch_seconds = n;
            confirm(group_id, settings, ConfigOption::LynchTimer, "SetLynchTimeA", raw_number(n))
        }
        Callback::SetToggle(toggle, on) => {
            settings.set(toggle, on);
            let answer = answer_label(toggle, on);
            confirm(group_id, settings, ConfigOption::Toggle(toggle), &toggle.answer_key(), answer)
        }

        Callback::ToggleRole(role) => {
            settings.toggle_role(role);
            show(role_menu(group_id, settings), None)
        }
        Callback::EnableAllRoles => {
            settings.enable_all_roles();
            show(role_menu(group_id, settings), None)
        }
        Callback::DisableAllRoles => {
            settings.disable_all_roles();
            show(role_menu(group_id, settings), None)
        }
        Callback::ValidateRoles => {
            let key = if settings.validate_roles(rng) { "RoleConfigValid" } else { "RoleConfigInvalid" };
            show(role_menu(group_id, settings), Some(Notice { message: Message::new(key), alert: true }))
        }
    }
}

fn show(screen: Screen, notice: Option<Notice>) -> CallbackOutcome {
    CallbackOutcome::Show { screen, notice }
}

/// A setting changed: a passing confirmation, then back to its submenu.
fn confirm(group_id: i64, settings: &GroupSettings, option: ConfigOption, key: &str, arg: Text) -> CallbackOutcome {
    show(
        submenu_screen(group_id, settings, option.submenu()),
        Some(Notice { message: Message::with(key, vec![arg]), alert: false }),
    )
}

fn raw_number(n: u32) -> Text {
    Text::Raw(n.to_string())
}

fn answer_label(toggle: Toggle, on: bool) -> Text {
    let (pos, neg) = toggle.wording().keys();
    Text::key(if on { pos } else { neg })
}

fn button(group_id: i64, label: Text, callback: Callback) -> Button {
    Button { label, action: ButtonAction::Callback(ConfigAction { group_id, callback }) }
}

/// Pairs buttons into rows of two, the last one alone if odd
/// (`GetConfigMenu`/`GetConfigSubmenu`'s `twoMenu`).
fn rows_of(buttons: Vec<Button>, width: usize) -> Vec<Vec<Button>> {
    buttons.chunks(width).map(|c| c.to_vec()).collect()
}

/// `UpdateHandler.cs:2169-2195`.
pub fn main_menu(group_id: i64) -> Screen {
    let mut buttons: Vec<Button> =
        Submenu::ALL.into_iter().map(|s| button(group_id, Text::key(s.key()), Callback::OpenSubmenu(s))).collect();
    buttons.push(button(group_id, Text::key("Done"), Callback::Done));
    Screen { message: Message::new("WhatToDo"), rows: rows_of(buttons, 2) }
}

/// `UpdateHandler.cs:2197-2250`, and the `RoleConfigInfo` text for the
/// role menu (1050-1054).
pub fn submenu_screen(group_id: i64, settings: &GroupSettings, submenu: Submenu) -> Screen {
    if submenu == Submenu::RoleConfig {
        return role_menu(group_id, settings);
    }
    let mut buttons: Vec<Button> = submenu_options(submenu)
        .into_iter()
        .map(|o| button(group_id, Text::key(o.key()), Callback::OpenOption(o)))
        .collect();
    buttons.push(button(group_id, Text::key("Back"), Callback::BackToMain));
    Screen { message: Message::new("WhatToDo"), rows: rows_of(buttons, 2) }
}

/// `UpdateHandler.cs:2252-2302`.
pub fn role_menu(group_id: i64, settings: &GroupSettings) -> Screen {
    let toggles: Vec<Button> = configurable_roles()
        .map(|role| {
            let mark = if settings.is_role_disabled(role) { "❌" } else { "✅" };
            let emoji = role.emoji().expect("configurable roles all have an emoji");
            button(group_id, Text::Raw(format!("{emoji} {mark}")), Callback::ToggleRole(role))
        })
        .collect();
    let mut rows = rows_of(toggles, 3);
    rows.push(vec![
        button(group_id, Text::key("EnableAllRoles"), Callback::EnableAllRoles),
        button(group_id, Text::key("DisableAllRoles"), Callback::DisableAllRoles),
    ]);
    let validate = if settings.roles_validated {
        button(group_id, Text::key("Valid"), Callback::Nothing)
    } else {
        button(group_id, Text::key("Validate"), Callback::ValidateRoles)
    };
    rows.push(vec![validate, button(group_id, Text::key("Back"), Callback::BackToMain)]);
    Screen { message: Message::new("RoleConfigInfo"), rows }
}

/// One option's question screen: its choices in a single column, then
/// Cancel.
pub fn option_screen(group_id: i64, settings: &GroupSettings, option: ConfigOption) -> Screen {
    let numbers = |values: &[u32], offset: u32, make: fn(u32) -> Callback| -> Vec<Button> {
        values.iter().map(|&v| button(group_id, raw_number(v + offset), make(v))).collect()
    };

    let (message, mut choices) = match option {
        // UpdateHandler.cs:1594-1608
        ConfigOption::Mode => (
            Message::with("GameModeQ", vec![Text::Raw(settings.mode.legacy_name().into())]),
            ModeSetting::ALL
                .into_iter()
                .map(|m| button(group_id, Text::key(m.button_key()), Callback::SetMode(m)))
                .collect(),
        ),
        // UpdateHandler.cs:1540-1559
        ConfigOption::MaxPlayer => (
            Message::with("MaxPlayersQ", vec![raw_number(settings.max_players)]),
            numbers(&MAX_PLAYER_CHOICES, 0, Callback::SetMaxPlayers),
        ),
        // UpdateHandler.cs:1622-1637
        ConfigOption::EndRoles => (
            Message::with("ShowRolesEndQ", vec![Text::Raw(settings.end_roles.legacy_name().into())]),
            EndRoles::ALL
                .into_iter()
                .map(|e| button(group_id, Text::key(e.button_key()), Callback::SetEndRoles(e)))
                .collect(),
        ),
        // UpdateHandler.cs:1817-1833
        ConfigOption::MaxExtend => (
            Message::with(
                "MaxExtendQ",
                vec![raw_number(DEFAULT_MAX_EXTEND_SECONDS as u32), raw_number(settings.max_extend_seconds)],
            ),
            numbers(&MAX_EXTEND_CHOICES, 0, Callback::SetMaxExtend),
        ),
        // UpdateHandler.cs:1651-1667
        ConfigOption::DayTimer => (
            Message::with(
                "SetDayTimeQ",
                vec![
                    raw_number(DEFAULT_DAY_SECONDS + DAY_DISPLAY_OFFSET),
                    raw_number(settings.day_seconds + DAY_DISPLAY_OFFSET),
                ],
            ),
            numbers(&DAY_CHOICES, DAY_DISPLAY_OFFSET, Callback::SetDayTimer),
        ),
        // UpdateHandler.cs:1681-1697
        ConfigOption::NightTimer => (
            Message::with("SetNightTimeQ", vec![raw_number(DEFAULT_NIGHT_SECONDS), raw_number(settings.night_seconds)]),
            numbers(&NIGHT_LYNCH_CHOICES, 0, Callback::SetNightTimer),
        ),
        // UpdateHandler.cs:1711-1727
        ConfigOption::LynchTimer => (
            Message::with("SetLynchTimeQ", vec![raw_number(DEFAULT_LYNCH_SECONDS), raw_number(settings.lynch_seconds)]),
            numbers(&NIGHT_LYNCH_CHOICES, 0, Callback::SetLynchTimer),
        ),
        // UpdateHandler.cs:1913-1979
        ConfigOption::Toggle(toggle) => {
            let (pos, neg) = toggle.wording().keys();
            (
                Message::with(&toggle.question_key(), vec![answer_label(toggle, settings.is_on(toggle))]),
                vec![
                    button(group_id, Text::key(pos), Callback::SetToggle(toggle, true)),
                    button(group_id, Text::key(neg), Callback::SetToggle(toggle, false)),
                ],
            )
        }
    };
    choices.push(button(group_id, Text::key("Cancel"), Callback::Cancel(option)));
    Screen { message, rows: rows_of(choices, 1) }
}

/// Every locale key this module can emit, for the locale coverage check.
pub fn all_locale_keys() -> Vec<String> {
    let mut keys: Vec<String> = [
        "WhatToDo",
        "Done",
        "ThankYou",
        "Back",
        "Cancel",
        "GroupCommandOnly",
        "GroupAdminOnly",
        "StartMe",
        "RoleConfigInfo",
        "EnableAllRoles",
        "DisableAllRoles",
        "Valid",
        "Validate",
        "RoleConfigValid",
        "RoleConfigInvalid",
        "GameModeQ",
        "GameModeA",
        "MaxPlayersQ",
        "MaxPlayersA",
        "ShowRolesEndQ",
        "ShowRolesEndA",
        "MaxExtendQ",
        "MaxExtendA",
        "SetDayTimeQ",
        "SetDayTimeA",
        "SetNightTimeQ",
        "SetNightTimeA",
        "SetLynchTimeQ",
        "SetLynchTimeA",
        FLEE_DISABLED_KEY,
    ]
    .into_iter()
    .map(String::from)
    .collect();
    keys.extend(Submenu::ALL.iter().map(|s| s.key().to_string()));
    keys.extend(ModeSetting::ALL.iter().map(|m| m.button_key().to_string()));
    keys.extend(EndRoles::ALL.iter().map(|e| e.button_key().to_string()));
    for submenu in Submenu::ALL {
        keys.extend(submenu_options(submenu).iter().map(|o| o.key().to_string()));
    }
    for toggle in Toggle::ALL {
        keys.push(toggle.question_key());
        keys.push(toggle.answer_key());
        let (pos, neg) = toggle.wording().keys();
        keys.push(pos.to_string());
        keys.push(neg.to_string());
    }
    keys.sort();
    keys.dedup();
    keys
}

// ---------- Callback data ----------

/// Why a callback string didn't decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl ConfigAction {
    /// A compact `"cfg|<group>|<code>[|<arg>...]"` string. The longest
    /// one (a toggle on a 14-digit negative group id) stays well under
    /// Telegram's 64-byte callback data limit.
    pub fn encode(&self) -> String {
        let body = match self.callback {
            Callback::OpenSubmenu(s) => format!("s|{}", submenu_index(s)),
            Callback::BackToMain => "b".to_string(),
            Callback::Done => "d".to_string(),
            Callback::OpenOption(o) => format!("o|{}", option_code(o)),
            Callback::Cancel(o) => format!("c|{}", option_code(o)),
            Callback::SetMode(m) => format!("m|{}", m.legacy_name()),
            Callback::SetMaxPlayers(n) => format!("mp|{n}"),
            Callback::SetEndRoles(e) => format!("er|{}", e.legacy_name()),
            Callback::SetMaxExtend(n) => format!("mx|{n}"),
            Callback::SetDayTimer(n) => format!("dt|{n}"),
            Callback::SetNightTimer(n) => format!("nt|{n}"),
            Callback::SetLynchTimer(n) => format!("lt|{n}"),
            Callback::SetToggle(t, on) => format!("t|{}|{}", toggle_index(t), on as u8),
            Callback::ToggleRole(r) => format!("r|{}", role_index(r)),
            Callback::EnableAllRoles => "ea".to_string(),
            Callback::DisableAllRoles => "da".to_string(),
            Callback::ValidateRoles => "v".to_string(),
            Callback::Nothing => "n".to_string(),
        };
        format!("cfg|{}|{body}", self.group_id)
    }

    pub fn decode(data: &str) -> Result<Self, DecodeError> {
        let err = || DecodeError(data.to_string());
        let parts: Vec<&str> = data.split('|').collect();
        if parts.len() < 3 || parts[0] != "cfg" {
            return Err(err());
        }
        let group_id: i64 = parts[1].parse().map_err(|_| err())?;
        let arg = |i: usize| parts.get(i).copied().ok_or_else(err);
        let num = |i: usize| -> Result<u32, DecodeError> { arg(i)?.parse().map_err(|_| err()) };
        let idx = |i: usize| -> Result<usize, DecodeError> { arg(i)?.parse().map_err(|_| err()) };

        let callback = match parts[2] {
            "s" => Callback::OpenSubmenu(*Submenu::ALL.get(idx(3)?).ok_or_else(err)?),
            "b" => Callback::BackToMain,
            "d" => Callback::Done,
            "o" => Callback::OpenOption(option_from_code(arg(3)?).ok_or_else(err)?),
            "c" => Callback::Cancel(option_from_code(arg(3)?).ok_or_else(err)?),
            "m" => Callback::SetMode(
                ModeSetting::ALL.into_iter().find(|m| m.legacy_name() == arg(3).unwrap_or("")).ok_or_else(err)?,
            ),
            "mp" => Callback::SetMaxPlayers(num(3)?),
            "er" => Callback::SetEndRoles(
                EndRoles::ALL.into_iter().find(|e| e.legacy_name() == arg(3).unwrap_or("")).ok_or_else(err)?,
            ),
            "mx" => Callback::SetMaxExtend(num(3)?),
            "dt" => Callback::SetDayTimer(num(3)?),
            "nt" => Callback::SetNightTimer(num(3)?),
            "lt" => Callback::SetLynchTimer(num(3)?),
            "t" => {
                let toggle = *Toggle::ALL.get(idx(3)?).ok_or_else(err)?;
                let on = match arg(4)? {
                    "1" => true,
                    "0" => false,
                    _ => return Err(err()),
                };
                Callback::SetToggle(toggle, on)
            }
            "r" => Callback::ToggleRole(*ALL_ROLES.get(idx(3)?).ok_or_else(err)?),
            "ea" => Callback::EnableAllRoles,
            "da" => Callback::DisableAllRoles,
            "v" => Callback::ValidateRoles,
            "n" => Callback::Nothing,
            _ => return Err(err()),
        };
        Ok(ConfigAction { group_id, callback })
    }
}

fn submenu_index(s: Submenu) -> usize {
    Submenu::ALL.iter().position(|&x| x == s).expect("in ALL")
}

fn toggle_index(t: Toggle) -> usize {
    Toggle::ALL.iter().position(|&x| x == t).expect("in ALL")
}

fn role_index(r: Role) -> usize {
    ALL_ROLES.iter().position(|&x| x == r).expect("in ALL_ROLES")
}

fn option_code(o: ConfigOption) -> String {
    match o {
        ConfigOption::Toggle(t) => format!("t{}", toggle_index(t)),
        other => other.key().to_string(),
    }
}

fn option_from_code(code: &str) -> Option<ConfigOption> {
    if let Some(i) = code.strip_prefix('t') {
        return Toggle::ALL.get(i.parse::<usize>().ok()?).copied().map(ConfigOption::Toggle);
    }
    [
        ConfigOption::Mode,
        ConfigOption::MaxPlayer,
        ConfigOption::EndRoles,
        ConfigOption::MaxExtend,
        ConfigOption::DayTimer,
        ConfigOption::LynchTimer,
        ConfigOption::NightTimer,
    ]
    .into_iter()
    .find(|o| o.key() == code)
}
