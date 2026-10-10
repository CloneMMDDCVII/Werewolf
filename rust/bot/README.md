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

Use `--config path/to/werewolf.toml` to keep the config elsewhere (relative
paths in it are then resolved from that file's folder). Set `RUST_LOG=debug`
for more detailed logs.

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
