Feature: An engine architecture where roles are easy to find, add, change and delete
  As the next maintainer of the Rust port
  I want every fact about a role in one file, one game loop that reads top to bottom,
  and one command that proves the code is shippable
  So that changing the game does not need a tour of eight files and a grep

  Behaviour does not change. Every scenario below is a structural guarantee;
  the 633 existing tests (historical-game replay, fixture replay, narration
  fuzz, bot end to end) are the proof that the game players get is identical.

  Background:
    Given the whole patch stack applied, ending with after-bot/0005
    And the architecture patches applied on top

  # ── Local CI ────────────────────────────────────────────────────────────

  Scenario: One command runs every check CI runs
    # not tested: process requirement, it is cargo xtask ci itself
    When a developer runs "cargo xtask ci" in rust/ on Linux, macOS or Windows
    Then it runs "cargo fmt --all --check"
    And then "cargo clippy --workspace --all-targets --locked -- -D warnings"
    And then "cargo test --workspace --locked"
    And it stops at the first failing step with a non-zero exit code
    And it prints each step's name and duration

  Scenario: GitHub Actions runs the same entry point
    # not tested: CI configuration in .github/workflows/rust.yml
    Given .github/workflows/rust.yml
    Then its check job runs "cargo xtask ci" and nothing else
    And it runs on pushes to every branch, not only master

  Scenario: An optional pre-push hook
    # not tested: developer tooling (cargo xtask install-hooks), no automated test
    When a developer runs "cargo xtask install-hooks"
    Then "git push" runs "cargo xtask ci" first and is refused if it fails
    And "git push --no-verify" skips it
    And "cargo xtask uninstall-hooks" removes only a hook xtask installed

  Scenario: The workspace is formatted and lint clean
    # not tested: checked by cargo xtask ci (rustfmt and clippy steps)
    Then rust/rustfmt.toml sets max_width = 120
    And "cargo fmt --check" and "cargo clippy -D warnings" both pass

  # ── One definition per role ─────────────────────────────────────────────

  Scenario: Every per-role fact lives in the role's own file
    # covered by: every_role_maps_to_its_own_definition
    Given a role, for example the Harlot
    Then game-engine/src/roles/harlot.rs declares its RoleDef:
      | fact                         | example for the Harlot        |
      | night question               | target pick, HarlotVisit      |
      | day question                 | none                          |
      | role info key                | RoleInfoHarlot                |
      | what a Seer sees             | itself                        |
      | cult conversion chance       | 70                            |
      | blocked when frozen          | yes                           |
      | balancing strength           | 6                             |
      | balancing group              | village                       |
      | how answers become actions   | its RoleBehavior impl         |
    And no other engine file matches on Role::Harlot to look up one of those facts

  Scenario: The compiler refuses a role without a definition
    # covered by: all_roles_covers_every_variant, every_role_maps_to_its_own_definition
    When a developer adds a variant to shared::Role
    Then the build fails until roles::def has an arm for it
    And a test fails unless ALL_ROLES lists it and def(role).role == role

  Scenario: Wolf membership is defined once
    # covered by: wolves_are_the_legacy_wolf_roles_and_the_snow_wolf
    Then shared::Role has is_eating_wolf (the legacy WolfRoles, Werewolf.cs:48)
    And shared::Role has is_wolf (WolfRoles plus the Snow Wolf)
    And no other crate defines its own wolf predicate or wolf role list

  Scenario: Mechanics that involve several roles stay in the night resolver
    # not tested: code structure; broken intra-doc links fail the cargo xtask ci docs step
    Given an interaction such as "the Harlot dies visiting the wolves' victim"
    Then it stays in its region of night.rs, in the legacy order
    And the role's file links to that region with an intra-doc link
    And "cargo xtask ci" fails if that link no longer resolves

  Scenario: Deleting a role is a short, compiler-guided job
    # not tested: compiler-enforced code structure
    When a developer deletes a variant from shared::Role
    Then every place still naming it is a compile error
    And deleting roles/<role>.rs and those references is the whole change

  # ── One game loop ───────────────────────────────────────────────────────

  Scenario: The main loop reads as the round order
    # not tested: code structure, reviewed by reading game.rs
    Given game.rs
    Then a Game struct owns the state that lives across rounds
      (alive, role states, lovers, kills, deaths, night memory, pending bites,
       the silver, cub and sandman flags, graves counted, last night's freeze)
    And run_game_with_rules builds a Game and loops night, day, second lynch, end check
    And no function in game.rs needs #[allow(clippy::too_many_arguments)]

  Scenario: There is one night resolver
    # not tested: code structure (the tie rule is checked by a_tied_wolf_vote_goes_to_whoever_is_listed_first)
    Then apply_night_results, resolve_night, resolve_night_by_actor and
      majority_target no longer exist
    And the tests that used them go through resolve_night_with_freeze or
      run_game_with_rules instead
    And a wolf-vote tie is decided only by night.rs (first in player order,
      Werewolf.cs:3246)

  # ── Readable surface ────────────────────────────────────────────────────

  Scenario: Comments describe the code, not its history
    # not tested: documentation-only
    Then no doc comment in game-engine calls the code a "proof-of-concept"
    And Werewolf.cs line citations are kept

  Scenario: A newcomer has a map
    # not tested: documentation-only
    Then rust/ARCHITECTURE.md describes the crates, the life of a game,
      and step-by-step how to add, change and delete a role
    And the Telegram bot's engine interfaces are unchanged:
      Presenter::ask_batch, Question::player/prompt/text, OnAnswer with
      closed and closed_key, NarrationEvent::Tell, Transform and GameSummary,
      and drunk wolves kept out of the night batch through skip
