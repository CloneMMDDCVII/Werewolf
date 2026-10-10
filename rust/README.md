# Werewolf for Telegram, in Rust

The Rust port of the bot. `ARCHITECTURE.md` explains how the crates fit
together and how to add, change or delete a role, `COMMANDS.md` tracks
which commands are ported, and `bot/README.md` is how to run the bot.

## Checks

One command runs everything CI runs, on Linux, macOS and Windows:

```
cargo xtask ci
```

That is `cargo fmt --all --check`, `cargo clippy --workspace --all-targets
--locked -- -D warnings`, `cargo test --workspace --locked`, then
`cargo doc` with warnings denied (a broken doc link fails), stopping at
the first failure. GitHub Actions runs the same command
(`.github/workflows/rust.yml`), so a green local run means a green CI run.
Set `WEREWOLF_TEST_POSTGRES_URL` to also run the storage tests against
Postgres, as CI does.

To run it automatically before every `git push`:

```
cargo xtask install-hooks      # cargo xtask uninstall-hooks to remove
```

Skip it for one push with `git push --no-verify`.
