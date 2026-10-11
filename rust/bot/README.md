# Werewolf for Telegram: running the bot

One program, one config file, no other software to install. It runs on
Linux, macOS and Windows.

## First run

1. Download the build for your machine and unzip it anywhere.
2. Run `werewolf-bot` (`werewolf-bot.exe` on Windows). The first time, it
   writes `werewolf.toml` next to itself and tells you what to do next.
3. Get a token from [@BotFather](https://t.me/BotFather) (send it `/newbot`)
   and put it in `werewolf.toml` as `token = "..."`, or set the
   `WEREWOLF_BOT_TOKEN` environment variable instead.
4. Run it again. It checks the token, creates `werewolf.db` and starts
   answering. Add the bot to a group and send `/startgame`.

Press Ctrl+C once to stop: no new games start, and running games finish
first. Press it again to end them at once. If the bot is killed outright,
the groups whose games were cut off are told so when it next starts.

## Settings

Everything lives in `werewolf.toml`; the file it writes explains each
setting. The ones you may want to change:

| Setting | Default | |
|---|---|---|
| `database` | `sqlite://werewolf.db` | A file next to the config. Use `postgres://user:pass@host/db` for a Postgres server. |
| `languages_dir` | `Languages` | The community language files. English is built in. |
| `global_admins` | none | Telegram user ids allowed to run bot-wide admin commands (`/killgame`, bans). |
| `language_admins` | none | Telegram user ids allowed to check and install language files (`/validatelangs`, `/uploadlang`). |
| `health_addr` | off | Serve a health check on this address, e.g. `127.0.0.1:8080`. See below. |

Global and language admin ids live only in this file. By design there is
no command or other in-band way to add one: to grant or revoke admin
rights, edit the file and restart the bot.

Use `--config path/to/werewolf.toml` to keep the config elsewhere (relative
paths in it are then resolved from that file's folder). Set `RUST_LOG=debug`
for more detailed logs.

## Operating the bot

Nothing here is needed to run the bot; it is for running it unattended on
a server.

### Health check

Set `health_addr = "127.0.0.1:8080"` in `werewolf.toml` (or the
`WEREWOLF_HEALTH_ADDR` environment variable, which wins) and the bot
answers `GET /healthz` on that address:

    $ curl -i http://127.0.0.1:8080/healthz
    HTTP/1.1 200 OK
    Content-Type: application/json
    ...
    {"healthy":true,"polling":true,"database":true,"running_games":3,"last_poll_secs":4}

It returns 200 when both checks pass and 503 when either fails:

- `polling`: the bot fetched updates from Telegram successfully in the
  last two minutes (a fetch normally takes up to 30 seconds).
  `last_poll_secs` is how long ago that was.
- `database`: the database answered a `SELECT 1` within three seconds.

`running_games` is for information and never makes the check fail. A
Telegram outage turns the check red without anything being wrong with the
bot, so alert on it rather than restarting on it. Use `0.0.0.0:8080` only
if the monitor is on another machine; the endpoint has no authentication.

### systemd

`deploy/werewolf-bot.service` is a ready-made unit for Linux; the comments
at its top say how to install it. It uses:

- `Type=notify`: the bot tells systemd it is ready once the database is
  open and polling has started.
- `WatchdogSec=180`: the polling loop itself reports in after every fetch
  from Telegram (about every 30 seconds), so if polling hangs systemd
  kills the bot, and `Restart=always` starts it again. Games cut off this
  way are announced when it comes back, as after any crash.
  A failed fetch reports in too, so a Telegram outage does not restart
  the bot; only a hung loop does. The health check's `polling` counts
  successful fetches only, so it is the signal for "can the bot reach
  Telegram", and the watchdog the one for "is the bot stuck".
- `KillSignal=SIGINT`: `systemctl stop` works like the first Ctrl+C, so
  running games finish first; whatever is still running after
  `TimeoutStopSec` is killed.

Outside a `Type=notify` unit (the `NOTIFY_SOCKET` environment variable is
unset), and on Windows, the bot sends systemd nothing.

### Logs

The bot logs to standard error, which systemd sends to the journal:

    journalctl -u werewolf-bot -f

`RUST_LOG` picks how much: `error`, `warn`, `info` (the default), `debug`
or `trace`, for everything or per module, e.g.
`RUST_LOG=info,bot::core=debug`. In the unit, set it with
`Environment=RUST_LOG=debug`.

## Beta and main bots

Run each from its own folder, with its own token and its own database. A
newer build upgrades the database it is given (backing up a SQLite file
first as `werewolf.db.bak-N`); an older build refuses to open a database
a newer one has upgraded, rather than damaging it. So never point the
main bot at the beta bot's database.

## Building from source

Install Rust from <https://rustup.rs>, then in `rust/`:

    cargo build --release -p bot

The program is `target/release/werewolf-bot`.
