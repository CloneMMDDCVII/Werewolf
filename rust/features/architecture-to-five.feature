Feature: Every dimension of the architecture assessment at 5/5
  Source: notes/rust-architecture-assessment.md. Behaviour stays identical:
  the 300-seed transcript gate keeps every digest unless a change is meant
  to alter play, and the bot's engine interfaces keep their paths.

  Scenario: Reliability - a panicking game frees its group
    # not tested: not built yet
    Given a game task that panics mid-night
    When the panic unwinds
    Then the group's slot is released by a Drop guard
    And the panic is logged with the group id
    And a new /startgame in that group succeeds (bot test)
    And shared std mutexes recover from poisoning instead of cascading

  Scenario: Resiliency - an operator can see and restart the bot
    # not tested: not built yet
    Given the bot runs under the shipped systemd unit
    Then it restarts on exit and reports readiness and liveness
    And an opt-in health endpoint reports polling, database and games
    And logs can be emitted as JSON

  Scenario: Security - no loose ends
    # not tested: not built yet
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
    # not tested: not built yet
    Then the release profile uses lto, one codegen unit and strip
