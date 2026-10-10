# Architecture

How the Rust port is put together, how one game runs through it, and how to
add, change or delete a role. `README.md` covers running the checks;
`COMMANDS.md` tracks which legacy commands are ported.

## Crates

Dependencies only point down this list:

| Crate | What it holds | Depends on |
|---|---|---|
| `shared` | `Role`, `Team`, `KillMethod`: identity the engine, bot and database all agree on (team, display key, emoji, wolf membership) | nothing |
| `i18n` | Loads the legacy `Languages/*.xml` files unchanged | `shared` |
| `db` | Postgres and in-memory storage | `shared` |
| `game-engine` | Every game rule, with no I/O | `shared` |
| `moderation` | Bans, restores, admin checks | `shared`, `db`, `i18n` |
| `control` | The lobby: joining, starting, group settings, dealing roles | `shared`, `game-engine`, `moderation`, `db`, `i18n` |
| `sim` | Text presenter, historical-game replays, fuzzing | `shared`, `game-engine`, `i18n` |
| `bot` | The Telegram bot | everything above except `sim` |
| `xtask` | `cargo xtask ci` and the pre-push hook | nothing |

`game-engine` never talks to Telegram. It asks questions and tells players
things through the `orchestrator::Presenter` trait; `bot` implements it for
Telegram, `sim` for transcripts and tests. `orchestrator/` is one file per
concern (`presenter.rs`, `prompt.rs`, `night_ask.rs`, `day.rs`,
`deaths.rs`, ...); its `mod.rs` re-exports them, so callers write
`orchestrator::resolve_day`, and this document names the file
(`orchestrator::day::resolve_day`) so you know where to look.

## The life of a game

1. `control::lobby::Lobby` collects players. `assign_roles` deals them
   roles with `game_engine::balancing`.
2. `game_engine::game::run_game_with_rules` builds a `Game`, the state that
   lives across rounds (who is alive, each role's `RoleState`, lovers,
   kills, the night's memory), and tells everyone their role.
3. Each round:
   - **Night.** `orchestrator::night_ask::resolve_night_with_freeze` asks every
     player their night question in one batch, then the roles that need
     the wolves' target (the Witch). Each role's `RoleBehavior` turns its
     answer into `NightAction`s. The Snow Wolf's freeze is applied
     (`freeze`), then `night::resolve_night_sequence` resolves the night
     region by region in the legacy order: arsonist, wolves, serial
     killer, cultist hunter, cult, chemist, harlot, seers, guardian angel,
     thief, witch. It returns deaths, transforms and private notes.
     `reveals` then tells the Seer, Sorcerer, Fool, Oracle and Augur what
     they saw.
   - **Deaths.** `Game::process_deaths_and_hunter_shots` records the
     deaths, kills lovers' partners (`orchestrator::deaths::resolve_lover_deaths`),
     asks dead Hunters for their shot (`orchestrator::deaths::resolve_hunter_shots`)
     and narrates each death (`death_text`).
   - **Day.** `orchestrator::day::resolve_day` asks the lynch vote and every day
     question together. `orchestrator::day::apply_day_results` and the game
     apply the Gunner, Detective, Spumpkin, Blacksmith, Sandman, Pacifist
     and Troublemaker, then the lynch.
   - **End check** after every step: an admin's `/killgame`, flees, the
     Tanner, then `game_end::check_game_end`.
4. The game ends with `NarrationEvent::GameOver` and a `GameSummary`.

## Where a role lives

Each role has one file, `game-engine/src/roles/<role>.rs`, with:

- `pub const DEF: RoleDef`: every fact the engine reads about the role,
  defined in `roles/definition.rs`. That covers its night and day
  question, its info text key, what the Seer and Sorcerer see, its cult
  conversion chance, whether a freeze stops it, its death lines and its
  balancing group, strength and pool.
- a type implementing `RoleBehavior`, which checks an answer and turns
  it into actions;
- the role's unit tests;
- a header saying what the role does and a "Resolved in" list linking to
  the code that carries it out. CI builds the docs with warnings denied,
  so a renamed function breaks those links loudly.

`roles::def(role)` maps every `Role` to its `DEF` with an exhaustive
match. A new `Role` variant does not compile until it has a file.

Mechanics where several roles meet (the Harlot visiting the wolves'
victim, a cult conversion against a Hunter, falling into a grave) stay
in `night.rs`, one method per legacy region. Splitting them per role
would scatter one rule across several files and lose the order the legacy
game resolves them in, which the replays depend on.

## Adding a role

1. **`shared/src/lib.rs`**: add the `Role` variant, add it to
   `ALL_ROLES`, and give it a `team()`, `locale_key()` and `emoji()`.
   Add it to `from_legacy_str` if the legacy database knows it, and to
   `EATING_WOLVES` or `is_wolf` if it is a wolf. The compiler and the
   `all_roles_tests` match list what is missing.
2. **`game-engine/src/roles/<role>.rs`**: copy `villager.rs` (no action)
   or the closest existing role, fill in `DEF`, implement `RoleBehavior`
   and add tests.
3. **`game-engine/src/roles/mod.rs`**: `pub mod <role>;` and a line in
   `def`.
4. **A new question?** Add a `Prompt` variant in
   `orchestrator/prompt.rs`; the compiler then asks for its legacy key in
   `Prompt::locale_key`, right below the enum. List it in `ALL_PROMPTS`
   too, then point `DEF.night` or `DEF.day` at it.
5. **Does the role touch other players at night?** Add a region to
   `night.rs` in the legacy order, plus any rule in `Night::visit`, and
   link it from the role's header.
6. **Text**: each key named in `DEF` must exist in
   `Werewolf for Telegram/Languages/English.xml`, or have English text in
   `messages::FALLBACK_TEXTS` (listed in `COMMANDS.md`) until it is added
   upstream. `sim/tests/locale_coverage.rs` fails on a key with neither.
7. Run `cargo xtask ci`.

## Changing a role

Change its `DEF` for what it is asked, what others see, its death lines
or its balancing. Change its `RoleBehavior` for which answers are valid.
Change the regions its header links to for what happens to others. The
historical-game replays (`sim/tests`) fail if the legacy outcome changes.
`sim/tests/engine_transcripts.rs` plays 300 seeded random games and
fails if any of them plays differently; a refactor should keep all 300,
and an intended rule change re-blesses the moved seeds (see that file).

## Deleting a role

Remove the `Role` variant, then follow the compiler: the role file, its
`mod` and `def` lines, `ALL_ROLES`, and any region or rule naming the
role. Keep a role the database still holds in `from_legacy_str` if old
games must still load, or keep the variant with `dealt: false` so it is
never dealt (as the Witch is). A role with `pool: Pool::Never` stays out
of balanced games but can still be dealt by a caller's own role pool
(Chef and Barkeep).

## Specs

The project's rule is that behaviour is agreed in Gherkin before it is
coded: each `.feature` file is the approval document for one piece of
work. They live in `features/` and `control/features/`, once, as
documentation; nothing executes them.

Directly under each `Scenario:` line, one comment says what checks it:

```gherkin
  Scenario: A tied vote kills nobody
    # covered by: a_tied_lynch_kills_nobody_and_says_so, tied_lynch_vote_kills_nobody
```

The names are `#[test]` functions in the workspace. `engine_transcripts`
means only the 300-game digest gate exercises the behaviour, and
`# not tested: <reason>` says why nothing does (operator-only, Telegram
layout, documentation, a CI step, or `gap` for a test still to write).
`sim/tests/specs_are_claimed.rs` fails when a scenario has none of these
or names a function that no longer exists, so a renamed or deleted test
shows up in the spec it was checking.
