# Command inventory & port status

Every `[Command(Trigger = "...")]` in the legacy `Werewolf Control` project
(`Werewolf for Telegram/Werewolf Control/Commands/*.cs`), listed once, with
where its logic actually lives today and how much of it this port covers.
Not a plan to port all 83 — most of these are Telegram-bot admin/infra
plumbing this rewrite doesn't need. Kept here so "have we looked at X" has
one answer instead of living in scattered session summaries.

Status legend: ✅ done · 🟡 partial (see notes) · ⬜ not started · ⛔ out of
scope (explained in notes, not just skipped silently)

## Gameplay (GameCommands.cs) — the core loop

| Command | Legacy source | Status | Notes |
|---|---|---|---|
| `join` | `Werewolf.cs::AddPlayer` (685-761) | ✅ | `control::lobby::Lobby::join` |
| `flee` | `Werewolf.cs::RemovePlayer` (829-869) | ✅ | Joining-phase leave: `Lobby::leave`. Running-game flee: `game_engine::game::apply_fled_players`, fed by `Presenter::drain_fled_players` (see below) |
| `forcestart` | `Werewolf.cs::ForceStart` (2517-2519) | ✅ | `Lobby::force_start` + `Lobby::try_start` |
| `startgame` | `Helpers.cs::StartGame(GameMode.Normal, ...)` | 🟡 | `Lobby` gets you to a validated roster; role *assignment* (`GameBalancing.Balance`) isn't ported — `run_game` still expects a roster with roles already picked |
| `startchaos` | `Helpers.cs::StartGame(GameMode.Chaos, ...)` | 🟡 | `Lobby::with_game_mode` tracks the mode; `GameMode::Chaos`'s actual effect on role selection (`GameBalancing.Balance(..., chaos: true, ...)`) is unported — same gap as `startgame` |
| `extend` | `Werewolf.cs::ExtendTime` (2521-2534) + `GameCommands.cs::Extend` (the `MaxExtend`/admin-bypass gating) | ✅ | `Lobby::extend_time` |
| `players` | `Werewolf.cs::ShowPlayers` | ⬜ | Pure read of `Lobby::players()`/`AlivePlayer` roster + a presenter call - thin, not yet wired to a real presenter |
| `stopwaiting` | DB-only (`NotifyGame` waitlist) | ⛔ | No game-logic content - a "notify me next game" DB row, belongs with whatever owns notifications, not `control`/`game-engine` |
| `nextgame` | DB-only (`NotifyGame` waitlist) | ⛔ | Same as `stopwaiting` |

## General (GeneralCommands.cs)

| Command | Status | Notes |
|---|---|---|
| `start` | ⬜ | PM-only onboarding/welcome message |
| `help` | ⬜ | Static help text, mostly an `i18n` lookup |
| `setlang` / `getlang` | ⬜ | Per-user language selection - `i18n::LanguagePack` already supports multiple packs, just needs a selection layer |
| `ping` | ⬜ | Trivial liveness check |
| `achv` | ⛔ | Achievement system not planned for this port (no achievement modeling anywhere in `game-engine`) |
| `chatid` / `changelog` / `runinfo` / `getstatus` / `version` | ⛔ | Bot infra/meta, no game logic |
| `stats` / `myidles` | ⛔ | Stats tracking - explicitly dropped per `db`'s module doc ("drop stats/achievement/website tables") |

## Admin (AdminCommands.cs) — group/global admin tools

All ⛔ for now: `smite`, `config`, `uploadlang`, `getban`, `validatelangs`,
`getidles`, `remlink`, `setlink`, `addach`, `remach`, `restore`,
`reviewgifs`, `fi`, `approvegifs`, `disapprovegifs`, `fixgifs`. Group
config (`config`) and moderation (`smite`, `getban`) are the two worth
revisiting first if this list gets picked back up - `moderation::Ban`
already models the data shape `getban`/bans would need, just no command
layer yet.

## Dev/infra (DevCommands.cs, GifCommands.cs, HelpCommands.cs)

All ⛔, no plan to port: bot-fleet management (`bangroup`, `build`,
`startnodes`, `stopnode`, `killnode`, `replacenodes`, `maintenance`,
`update`), gif review pipeline (`dumpgifs`, `learngif`, `donate*`,
`customgif`), and one-off ops/debug tools (`sql`, `whois`, `usage`,
`checkgroups`, `getlogs`, `clearlogs`, `broadcast`, `test`, `getroles`,
`skipvote`, `playtime`, `winchart`, `asplode`, `notifyspam`, `notifyban`,
`ohaider`, `movelang`, `commitlangs`, `resetlink`, `user`, `unlockbeta`,
`lockbeta`, `preferred`, `leavegroup`, `reloadenglish`, `clearcount`,
`sendonline`, `adddonation`, `updatestatus`, `getbans`, `permban`,
`remban`, `getcommands`, `grouplist`, `rolelist`). `killgame`
(`DevCommands.cs:237`, admin-only force-end of a running game) is the one
exception worth flagging: it's the in-game counterpart to `Lobby::kill`
(joining-phase only) and needs the same cancellation seam described below
for running-game flee - tracked, not forgotten.

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
`killgame`'s "end this game outright, no winner" would be the natural
next thing built on the same polling pattern, not yet added.
