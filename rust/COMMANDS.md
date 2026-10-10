# Command inventory & port status

Every `[Command(Trigger = "...")]` in the legacy `Werewolf Control` project
(`Werewolf for Telegram/Werewolf Control/Commands/*.cs`), listed once, with
where its logic actually lives today and how much of it this port covers.
Not a plan to port all 83 — most of these are Telegram-bot admin/infra
plumbing this rewrite doesn't need. Kept here so "have we looked at X" has
one answer instead of living in scattered session summaries.
How the engine is organised, and how to add a role, is in
`ARCHITECTURE.md`.

Status legend: ✅ done · 🟡 partial (see notes) · ⬜ not started · ⛔ out of
scope (explained in notes, not just skipped silently)

## Gameplay (GameCommands.cs) — the core loop

| Command | Legacy source | Status | Notes |
|---|---|---|---|
| `join` | `Werewolf.cs::AddPlayer` (685-761) | ✅ | `control::lobby::Lobby::join` |
| `flee` | `Werewolf.cs::RemovePlayer` (829-869) | ✅ | Joining-phase leave: `Lobby::leave`. Running-game flee: `game_engine::game::apply_fled_players`, fed by `Presenter::drain_fled_players` (see below) |
| `forcestart` | `Werewolf.cs::ForceStart` (2517-2519) | ✅ | `Lobby::force_start` + `Lobby::try_start` |
| `startgame` | `Helpers.cs::StartGame(GameMode.Normal, ...)` | ✅ | `Lobby` → `Lobby::assign_roles` (`game_engine::balancing::balance`) → `run_game`, end to end |
| `startchaos` | `Helpers.cs::StartGame(GameMode.Chaos, ...)` | ✅ | Same path, `Lobby::with_game_mode(GameMode::Chaos)` |
| `extend` | `Werewolf.cs::ExtendTime` (2521-2534) + `GameCommands.cs::Extend` (the `MaxExtend`/admin-bypass gating) | ✅ | `Lobby::extend_time`; the cap and the group's `AllowExtend` come from `GroupSettings` via `Lobby::from_group_settings` |
| `players` | `Werewolf.cs::SendPlayerList`/`OutputPlayers` (1352-1427) | ✅ | Joining phase: `Lobby::players()` (just names). Running game: `game_engine::player_list::render_player_list` |
| `stopwaiting` | DB-only (`NotifyGame` waitlist) | ⛔ | No game-logic content - a "notify me next game" DB row, belongs with whatever owns notifications, not `control`/`game-engine` |
| `nextgame` | DB-only (`NotifyGame` waitlist) | ⛔ | Same as `stopwaiting` |

## General (GeneralCommands.cs)

| Command | Status | Notes |
|---|---|---|
| `start` | ✅ | `bot`: registers the player as reachable for PMs, takes a `join<group>` payload from the Join button, otherwise a welcome |
| `help` | ✅ | `bot`: the legacy links, only for a bare `/help`. `/rolelist` dropped from it, as that command is ⛔ |
| `setlang` | ✅ | `control::language_picker`: base then variant picker shared with the group `Lang` option, stored via `db::Players::set_language`. Spec: `control/features/language_picker.feature` |
| `getlang` | ✅ | `bot::menus`: `/getlang <file>` sends that file; bare `/getlang` offers bases then variants as buttons, each menu led by "All", a zip of what it lists |
| `ping` | ✅ | `bot`: sends, then edits in the round-trip time (`Ping2`) |
| `achv` | ⛔ | Achievement system not planned for this port (no achievement modeling anywhere in `game-engine`) |
| `chatid` / `changelog` / `runinfo` / `getstatus` / `version` | ⛔ | Bot infra/meta, no game logic |
| `stats` / `myidles` | ⛔ | Stats tracking - explicitly dropped per `db`'s module doc ("drop stats/achievement/website tables") |

## Admin (AdminCommands.cs) — group/global admin tools

Scopes: `notes/scopes/moderation.feature` and `language-admin.feature` in the project folder. Every
moderation command here is pure logic in the `moderation` crate (no Telegram, no
SQL), run over storage by `moderation::Moderation` (`db::Store`).

| Command | Legacy source | Status | Notes |
|---|---|---|---|
| `smite` | `AdminCommands.cs:26-63` → `Werewolf.cs::FleePlayer` (5357-5391) | ✅ | A forced flee. `moderation::smite_targets` picks the ids (reply, mentions, one typed id); joining: `Lobby::leave`; running game: `Presenter::drain_fled_players`, the same path as `/flee`. Skips "allow flee", silent for dead or absent targets. A player reported twice in one window now dies once (was a real double-death bug in `apply_fled_players`). The C#'s "cannot flee while initializing" can't happen here: `try_start` and `assign_roles` leave no gap |
| `getban` | `AdminCommands.cs:131-158`, target via `Extensions.cs::GetTarget` (136-185) | ✅ | `moderation::getban_report` + `resolve_target`. Changes: an expired ban reads "Not banned" (the C# showed negative time until the next refresh); reasons are HTML-escaped |
| `getidles` | `AdminCommands.cs:220-271`, `werewolf.sql` `Get[Group]IdleKills24Hours` | ✅ | `moderation::idles` + `db::Games::idle_kills_since` |
| `restore` | `AdminCommands.cs:496-581`, `UpdateHandler.cs:1125-1152`, `werewolf.sql` `RestoreAccount` | ✅ | `moderation::restore` scores the pair; `db::Games::move_player_history` moves games and kills. No row ids to compare, so "old is newer" checks Telegram ids only; empty histories no longer crash it. Achievement merge dropped with achievements. The yes/no buttons (dev-only confirm) are the presenter's |
| `config` | `AdminCommands.cs:66-100`, `UpdateHandler.cs` 1026-1985 and 2169-2300, `Database/GroupConfig.cs` | ✅ | `control::group_config`: typed `GroupSettings` persisted per group through `db::Store` (JSON), the full menu (Timers, RoleConfig, Mechanics, GroupSettings with `Lang`) as Telegram-free `control::menu::Screen`s, role toggles with Validate, compact callback data. Spec: `control/features/group_config.feature`. Lobbies take max players, mode, max extend, allow extend, disabled roles and burning overkill from it (`Lobby::from_group_settings`). Games take the rest through `control::game_rules::game_start` (`GameRules`: timers, secret lynch with votes/voters, show roles on death, end-of-game roles, thief full; Random mode rolls them per game, see "Game mechanics"). Show IDs, shuffle player list and NSFW are display-only and read by the presenter from `GroupSettings`. Dropped: `Allow{Tanner,Fool,Cult,Thief,Arsonist}`, unreachable in the legacy menu and never read; the Random language variant, never published |
| `setlink` / `remlink` | `AdminCommands.cs:273-322` | ✅ | `group_config::run_setlink_command`/`run_remlink_command`, same validation and hardcoded replies. Stored as `GroupSettings::link`; `linked_group_name` renders it for the "you joined" PM (`Werewolf.cs:741`). Unlike the legacy commands, a link set before the group was recorded is kept |
| `validatelangs` | `AdminCommands.cs:161-218`, `LanguageHelper.cs:64-178` | ✅ | `i18n::admin`: `check_file` (language node, missing strings, placeholders, GIF caption length, `/join`, ads), `file_report`, `summary_report`, `bases` for the menu. Changes: every duplicated key is flagged (the C# only looked at two), ads are checked per file in the summary, checks see the raw value with markup, and the C#'s callback-length checks are gone because buttons carry a short `i18n::LanguageId` instead of names (`AdminButton` is the menus' callback codec) |
| `uploadlang` | `AdminCommands.cs:102-129`, `LanguageHelper.cs:180-430` | ✅ | `i18n::admin`: `check_upload_name`, `check_upload` (clash with installed files, comparison with the file it replaces), `install` (the "New" button). Dropped: committing English.xml to git from the bot, the `Language` DB table, node reload broadcasts; the caller reloads its `Catalog`. Wired in `bot::lang_admin` for `language_admins` and global admins; an install reloads the bot's languages for the next game |
| `addach` / `remach` | `AdminCommands.cs:324-494` | ⛔ | Achievements are dropped |
| `reviewgifs` / `approvegifs` / `disapprovegifs` / `fixgifs` | `AdminCommands.cs:583-900` | ⛔ | Donation gifs; donations return later as Telegram Stars |
| `fi` | `AdminCommands.cs:671-684` | ⛔ | Bot run info, infra |

Also in `moderation`, used by every command above and the gameplay ones:

- **Permission gate** (`moderation::gate::check`, `UpdateHandler.cs:327-440`): banned senders are dropped silently, then anonymous/channel senders, blocked support chats, dev / lang admin / global admin / group-only / group admin, in the C#'s order. `policy_for(trigger)` holds each ported command's `[Command(...)]` flags. Not ported: the flood counter (`AddCount`), which is transport rate limiting.
- **Bans** (`moderation::BanList`, persisted via `db::Bans`): one ban in force per player, history kept in storage, legacy `GlobalBan` rows convert with `SqlDateTime.MaxValue` as permanent.

## Dev/infra (DevCommands.cs, GifCommands.cs, HelpCommands.cs)

All ⛔, no plan to port: bot-fleet management (`bangroup`, `build`,
`startnodes`, `stopnode`, `killnode`, `replacenodes`, `maintenance`,
`update`), gif review pipeline (`dumpgifs`, `learngif`, `donate*`,
`customgif`), and one-off ops/debug tools (`sql`, `whois`, `usage`,
`checkgroups`, `getlogs`, `clearlogs`, `broadcast`, `test`, `getroles`,
`skipvote`, `playtime`, `winchart`, `asplode`, `notifyspam`, `notifyban`,
`ohaider`, `movelang`, `commitlangs`, `resetlink`, `user`, `unlockbeta`,
`lockbeta`, `preferred`, `leavegroup`, `reloadenglish`, `clearcount`,
`sendonline`, `adddonation`, `updatestatus`, `getbans`,
`getcommands`, `grouplist`, `rolelist`).

`permban` and `remban` (`DevCommands.cs:848-1055`, global admin) are the
exceptions and are ✅: they are how bans get issued and lifted, so they
live with the rest of moderation (`moderation::permban` / `remban`,
stored through `Moderation`). A new ban replaces the player's previous
one in memory and adds to their history in storage; the reason is every
non-target word (the C# dropped the first word whatever it was). Lifting
keeps the row, marked lifted. `getbans` stays ⛔: it dumps the in-memory
spam-filter list, which isn't ported.

`killgame` (`DevCommands.cs:237`, admin-only force-end of a running
game) is the one exception, and it's ✅ now: `Presenter::game_was_killed`,
polled by `run_game` at the same three checkpoints as
`drain_fled_players`, ends the game immediately with the new
`WinOutcome::Killed` (deliberately "no winner," not "we don't know the
winner" like `RngDependent`/`InsufficientData`/`Unimplemented` - the
source's own `Kill()` sends a literal `"Game removed"`, not a
`GetLocaleString` call, so `WinOutcome::Killed` has no locale key at
all, checked directly rather than assumed).

## The seam behind the ✅ running-game entries

`game_engine::run_game` had no way to react to anything from outside its
own night/day loop - a player leaving, an admin killing the game - until
now. `Presenter::drain_fled_players` is the seam: a new default-empty
method any presenter can override to report players who asked to leave
since it was last polled. `run_game` polls it at three checkpoints per
round (start of round, after night resolves, after day resolves - the
same points it already checks for a win) and feeds whatever comes back
through `game::apply_fled_players`, which removes them, records the
death as `KillMethod::Flee`, and runs the same `resolved_winner` check
every other death triggers. Not truly instant (a flee lands within one
of those three windows, not mid-question) - an honest approximation,
not a silent one, documented on `drain_fled_players` itself.

`Presenter::game_was_killed` is the same pattern applied to `killgame`:
another default-`false` method, polled right alongside
`drain_fled_players` at the same three checkpoints. A `true` short-
circuits everything else that checkpoint would have done (fled players,
win checks) and returns immediately with `WinOutcome::Killed` - an
admin ending the game outright takes priority over any death that
would otherwise resolve the same instant.

## The seam behind `startgame`/`startchaos`'s ✅

`game_engine::balancing` (`Shared/GameBalancing.cs`) picks which role
each joined player gets: 1-5 wolves scaled to player count, one of
every other legacy role added to a candidate pool, shuffled, truncated
to the roster size, a few structural fixups (no wolf-dependent role
without a wolf, no Cultist without a CultistHunter, no Apprentice Seer
without a Seer), then checked against team-size and (outside `chaos`
mode) strength-variance rules before accepting - retried up to 500
times. `Lobby::assign_roles` is the glue: calls `balance`, zips the
result against the joined roster, hands back the `Vec<AlivePlayer>`
`run_game` takes directly. `Chef`/`Barkeep`/`Witch` (roles this port
added with no legacy precedent) never appear - `GameBalancing.cs`
predates all three, so there's no real strength value or balancing
rule to assign them by.

## The seam behind `players`'s ✅

`game_engine::player_list::render_player_list` ports the content-half of
`SendPlayerList` (1352-1408) - the actual `/players` command
(`GameCommands.cs::Players` -> `GameInfo.ShowPlayers()` ->
`Werewolf.cs::OutputPlayers()`) doesn't rebuild the list at all, it just
re-points to whatever player-list message is already being kept live,
throttled to once per 10 seconds - that Telegram-message-editing
plumbing is presentation, not ported. What *is* ported is "given the
roster and who's died how, what does each line say": alive players show
as alive; dead ones show `Fled` vs `Dead` (recovered from
`KillMethod::Flee` rather than tracked as a separate flag) and, only if
the group reveals roles on death, their role and love status - checked
directly against the source, where that reveal clause is nested
*inside* the `ShowRolesDeath` condition, not shown unconditionally. The
`ShufflePlayerList` anonymized-order variant (plus its Valentine's Day
love-emoji easter egg) is a cosmetic alternate rendering, not part of
"what does the list say," and isn't ported.

## Game mechanics (not commands)

Mechanics the briefing listed as unported, scoped in
`features/finish-game-mechanics.feature`:

| Mechanic | Status | Where |
|---|---|---|
| Snow Wolf freeze (same-night block, no repeat target, GA block, Hunter/SK visits) | ✅ | `game_engine::freeze` |
| Snow Wolf turns Wolf when the last wolf dies | ✅ | `game::apply_transforms` |
| Seer, Sorcerer, Fool, Oracle readings with legacy disguises, read mid-night after the Harlot (a reader killed earlier learns nothing) | ✅ | `game_engine::reveals::resolve_night_readings`, called from `night::Night::seers` |
| Augur readings (needs the balancer's pool: `balance_with_pool` + `run_game_with_role_pool`) | ✅ | same |
| Detective snoops by day, wolves may catch them | ✅ | `reveals::resolve_detective` |
| Troublemaker's second lynch; Pacifist vs Troublemaker, last one wins | ✅ | `game::run_game`, `orchestrator::last_day_veto` |
| Lynch tie / no votes announcements (no re-vote: `RandomLynch` is `false`) | ✅ | `orchestrator::LynchResult` |
| Clumsy Guy vote redirect | ✅ | `orchestrator::LynchVotes` |
| Revealed Mayor's vote counts double on every later lynch | ✅ | `orchestrator::tally_lynch` |
| Hunter domino chains; a last shot skipped (`HunterSkipChoice*`) told apart from one never taken (`HunterNoChoice*`) | ✅ | `game::process_deaths_and_hunter_shots`, `orchestrator::resolve_hunter_shots` |
| Gunner bullet count shown and carried over a Thief steal | ✅ | `roles::gunner::bullets_left`, `night::Night::thief` |
| Thief full mode: a try every night, 50% to land, never on wolves or Cultists, victim becomes the Thief, frozen Thief blocked, SK/Grave Digger/away Harlot or GA | ✅ | `night::Night::thief` |
| Cultist Hunter kills a hunted Cultist (before the cult converts), reports misses and dead targets, dies at the SK | ✅ | `night::Night::cultist_hunter` |
| The night in the legacy order, through one shared visit rule (Arsonist, wolves, SK, Cultist Hunter, cult, Chemist, Harlot, seers, GA, Thief) | ✅ | `game_engine::night`. Scope: `features/night-sequence.feature` |
| The Arsonist offered only undoused houses, shown those doused (`AlreadyDousedList`), spark only once something is doused | ✅ | `orchestrator::resolve_night_with_freeze` |
| The Grave Digger told whose graves they dig (`DigGraves`/`DigNoGraves`) | ✅ | `game::run_game_with_rules` |
| Day abilities announced when used; the Pacifist's peace closes the vote | ✅ | `orchestrator::DayAnswers` (`OnAnswer::closed`). Scope: `features/day-abilities.feature` |
| The Gunner's shot kills at once: told when answered, the victim's questions close, votes by or for them dropped. Their lover's grief, a shot Hunter's last shot and role changes still wait for the day's questions to close | ✅ | `orchestrator::DayAnswers::shoot` |
| A Gunner or Spumpkin who kills the Wise Elder becomes a Villager (`GunnerShotWiseElder`, `DetonatedWiseElder`) | ✅ | `orchestrator::DayAnswers::shoot`, `game::run_game_with_rules` |
| Role info and team lists when roles are dealt; every role change told to the player and their old and new teams; the Doppelganger's role model, picked at random for a Wild Child or Doppelganger who didn't | ✅ | `game_engine::role_info`. Scope: `features/role-info.feature` |
| Game length at the end of the summary (`EndTime`) | ✅ | `bot::game` (the presenter keeps the clock) |
| Achievements | ⛔ | Dropped for this port |

Group settings the engine follows, scoped in `features/game-rules.feature`
(`run_game_with_rules`; `run_game` keeps the legacy defaults):

| Setting | Status | Where |
|---|---|---|
| Day/night/lynch timers announced (`NightTime`, `DayTime` + `Day`, `LynchTime`), longer first night and bigger-game days | ✅ | `rules::GameRules::{night_seconds, day_seconds}`. The countdown is the presenter's (`bot::game`) |
| Public lynch votes announced as cast | ✅ | `orchestrator::LynchVotes`, called by the presenter as each vote lands (`Presenter::ask_batch`) |
| Secret lynch: vote counts only, then results with votes and optionally voters | ✅ | `orchestrator::secret_lynch_results` |
| Show roles on death | ✅ | `NarrationEvent::Death::reveal_role` |
| End-of-game summary per `ShowRolesEnd` | ✅ | `summary::GameSummary`, `NarrationEvent::GameSummary` |
| Random mode rolls (mode, thief full, secret lynch, votes, voters, roles on death, burning overkill, end roles) | ✅ | `control::game_rules::game_start`. Divergence: the rolled roles-on-death and end-roles values are used; legacy rolled them and then read the stored flags |
| Show IDs, shuffle player list, NSFW | ⛔ engine | Display-only, read by the presenter |

## The bot itself (`rust/bot`)

The Telegram front end: `werewolf-bot`, one binary plus `werewolf.toml`
(see `bot/README.md`). Commands above marked ✅ are wired to Telegram
there, through the permission gate, with:

- **Join timer** (`bot::join_timer`, `GameTimer` in Werewolf.cs): three
  minutes, reminders at 60/30/10 seconds, joins announced every 30
  seconds, late joins buying time back, `/extend` capped at five minutes.
- **One game per group**, a player in one game at a time, new games
  refused when memory is short (`control::capacity`) or the bot is
  shutting down.
- **Running games** (`bot::game`): what the group's settings say
  (`control::game_rules::game_start`, Random mode included) and roles
  dealt with their pool (`balance_with_pool`), roles and team lists by PM
  as the engine writes them (`game_engine::role_info`), wolf and cult
  prompts naming who to agree with, the Mayor's, Pacifist's and
  Arsonist's single Reveal/Peace/Spark button, the player list posted as
  each night, day and vote starts (shuffled and with IDs when the group
  says so), each phase's
  questions sent together under the engine's announced timer (the
  Snow Wolf's freeze with the rest of the night; day abilities open for
  the whole day, then the vote), lynch votes announced as they land,
  buttons closed with `ChoiceAccepted` or `TimesUp`, every engine
  message, note and the end-of-game summary rendered from the group's
  language with the game's length, and every game, death and final
  role recorded through `db::Games`.
- **Restarts**: games cut off by a crash are announced to their groups
  on the next start and closed as interrupted.

Tested end to end without a token: `bot/tests/bot.rs` drives the real
core, games and an in-memory database against `bot::fake::FakeTelegram`
on a paused clock.

Known gaps:

- In-game night and role texts still to port are listed by owner in the
  project's `notes/ui-audit.md`.

## Locale keys missing from English.xml

The translation files are left as they are. These keys are used by the
engine but don't exist in `Werewolf for Telegram/Languages/English.xml`,
so the engine renders English fallback text for them
(`game_engine::messages::FALLBACK_TEXTS`). Once a key is added to the
files, delete its fallback: `sim/tests/locale_coverage.rs` fails until
you do, so the files' text and translations take over.

| Key | Used for | Placeholders |
|---|---|---|
| `Detonation` | A Spumpkin's detonation (death line) | `{0}` Spumpkin, `{1}` victim, `{2}` "{victim} was {role}" when roles are shown |
| `DetonatedWiseElder` | A Spumpkin who blew up the Wise Elder | `{0}` Spumpkin, `{1}` Wise Elder |
| `RoleInfoSpumpkin` | The Spumpkin's role info | none |
| `RoleInfoWitch` | The Witch's role info (new role, not in the legacy game) | none |
| `RoleInfoChef` | The Chef's role info (beta-fork role, no mechanics here) | none |
| `RoleInfoBarkeep` | The Barkeep's role info (beta-fork role, no mechanics here) | none |

Questions with no key at all (the bot's own English, `bot::game::question_text`):
the Witch's heal and poison, and the Spumpkin's detonation (the legacy menu
used `AskDetonate`, Werewolf.cs:5128, which English.xml doesn't have).
