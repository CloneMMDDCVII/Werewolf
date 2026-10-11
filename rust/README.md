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
`cargo doc` with warnings denied (a broken doc link fails), then
`cargo deny check advisories` (RustSec advisories against `Cargo.lock`,
policy in `deny.toml`), stopping at the first failure. GitHub Actions runs
the same command (`.github/workflows/rust.yml`) with cargo-deny installed.
Locally the advisory check runs only if `cargo-deny` is on your PATH
(`cargo install --locked cargo-deny`); otherwise it is skipped and the run
ends by saying so, so a green local run without it does not cover
advisories.
Set `WEREWOLF_TEST_POSTGRES_URL` to also run the storage tests against
Postgres, as CI does.

To run it automatically before every `git push`:

```
cargo xtask install-hooks      # cargo xtask uninstall-hooks to remove
```

Skip it for one push with `git push --no-verify`.
