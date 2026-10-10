Feature: Every dimension of the architecture assessment at 5/5
  Source: notes/rust-architecture-assessment.md. Behaviour stays identical:
  the 300-seed transcript gate keeps every digest unless a change is meant
  to alter play, and the bot's engine interfaces keep their paths.

  Scenario: Reliability - a panicking game frees its group
    # covered by: a_game_that_panics_frees_its_group_and_says_why
    Given a game task that panics mid-night
    When the panic unwinds
    Then the group's slot is released by a Drop guard
    And the panic is logged with the group id
    And a new /startgame in that group succeeds (bot test)
    And shared std mutexes recover from poisoning instead of cascading

  Scenario: Resiliency - an operator can see and restart the bot
    # covered by: healthy_needs_recent_polling_and_the_database, the_endpoint_reports_over_http, states_arrive_as_datagrams_on_the_socket, an_unreachable_database_is_asked_twice_for_interrupted_games
    Given the bot runs under the shipped systemd unit
    Then it restarts on exit, reports readiness, and feeds the watchdog from the polling loop
    And an opt-in health endpoint reports polling, database reachability and running games
    And listing interrupted games at startup is retried once if the database fails
    And logs go to journald through RUST_LOG, with no separate JSON format

  Scenario: Security - no loose ends
    # covered by: a_database_under_a_path_with_a_quote_is_backed_up, oversized_uploads_are_refused, only_owner_only_modes_keep_the_token_private
    Then VACUUM INTO refuses or escapes a path with a quote
    And /uploadlang rejects files over a size cap
    And startup warns when the config file holding the token is group- or world-readable
    And cargo deny check advisories runs in CI
    And bot/README.md says admin ids live only in the config file

  Scenario: Understandability - a file per concern
    # not tested: not built yet
    Then orchestrator.rs is split into presenter, prompt, night asking, day and deaths modules
    And game.rs keeps the Game loop while transforms move to their own module
    And every existing public path still resolves through re-exports
    And NightAction docs describe the code, not its history

  Scenario: Extensibility - a new question is one edit
    # not tested: not built yet
    Then each Prompt declares its own locale key in one place

  Scenario: Maintenance - less hand-written test scaffolding
    # not tested: not built yet, apart from the .feature files part (specs_are_claimed)
    Then game-engine exports a shared test presenter and night scene builder
    And tests touched by this work use them
    And the .feature files exist once, as documentation linked from ARCHITECTURE.md
    And ARCHITECTURE.md explains re-blessing digests after a rename

  Scenario: Performance stays at 5
    # not tested: a build setting in rust/Cargo.toml
    Then the release profile uses lto, one codegen unit and strip
