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
| `startgame` | `Helpers.cs::StartGame(GameMode.Normal, ...)` | ✅ | `Lobby` → `Lobby::assign_roles` (`game_engine::balancing::balance`) → `run_game`, end to end |
| `startchaos` | `Helpers.cs::StartGame(GameMode.Chaos, ...)` | ✅ | Same path, `Lobby::with_game_mode(GameMode::Chaos)` |
| `extend` | `Werewolf.cs::ExtendTime` (2521-2534) + `GameCommands.cs::Extend` (the `MaxExtend`/admin-bypass gating) | ✅ | `Lobby::extend_time` |
| `players` | `Werewolf.cs::SendPlayerList`/`OutputPlayers` (1352-1427) | ✅ | Joining phase: `Lobby::players()` (just names). Running game: `game_engine::player_list::render_player_list` |
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
`remban`, `getcommands`, `grouplist`, `rolelist`).

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
