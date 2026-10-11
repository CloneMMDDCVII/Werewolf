# Scope: run the Rust engine as a real Telegram bot, with persistent storage.
#
# Decisions (Pierre asked for cross-platform and easy setup, and left the
# choice to us):
#   - Telegram: teloxide, long polling. No public host, domain, TLS or webhook;
#     the bot runs anywhere that can reach api.telegram.org.
#   - Database: one storage layer (sqlx) with two backends, picked by the
#     database URL in werewolf.toml:
#       sqlite://werewolf.db   default. Embedded, nothing to install. For
#                              self-hosters and development.
#       postgres://...         for the main bot at legacy scale (15M players at
#                              peak). Backups, replicas and access from more
#                              than one machine.
#     Both run the same tests in CI. This keeps the Postgres plan in
#     rust/db/src/lib.rs for production and adds SQLite for easy setup.
#   - Scale: one bot token, one process, long polling. Pierre confirmed one
#     token has enough throughput, and bots are no longer split for load.
#   - Beta and main are separate bots with separate databases. Sharing one would
#     break main: a beta build's migrations would make main refuse to start
#     (see "An older bot refuses a newer database").
#   - One binary, `werewolf-bot`, in a new `bot` crate. English.xml is embedded
#     in it; the other 215 language files are optional, loaded from a folder.
#   - Config: one file, werewolf.toml. The token can come from the
#     WEREWOLF_BOT_TOKEN environment variable instead.
#
# Out of scope (later threads):
#   - Importing the legacy SQL Server data (bans, players, game history).
#   - Group config menus, moderation commands and the missing game mechanics.
#     Other threads own those; this scope gives them storage to persist into.
#   - /setlang, /help, donations, stats and achievements.
#   - Surviving a restart mid-game. A restart ends running games, as in legacy.
#
# Engine change (touches game-engine/src/orchestrator.rs, which the
# game-mechanics thread is also editing):
#   Today run_game awaits one player's question before asking the next, so with
#   real timers a 10-player night or lynch would take 10 x the timer. This scope
#   adds a batched ask to the Presenter trait whose default implementation asks
#   one by one, exactly as now, so every existing presenter and test is unchanged.

Feature: Setting the bot up

  Scenario: First run with no config writes a template and explains what to do
    # covered by: first_run_writes_a_commented_template_and_explains
    Given there is no werewolf.toml next to the binary
    And WEREWOLF_BOT_TOKEN is not set
    When I run werewolf-bot
    Then a werewolf.toml with every setting commented is created
    And the bot prints where the file is and that it needs a token from @BotFather
    And it exits with a non-zero code

  Scenario: The token can come from the environment
    # covered by: the_token_can_come_from_the_environment
    Given werewolf.toml has no token
    And WEREWOLF_BOT_TOKEN is set to a valid token
    When I run werewolf-bot
    Then the bot connects with the token from the environment

  Scenario: The environment variable wins over the file
    # covered by: the_environment_wins_over_the_file
    Given werewolf.toml and WEREWOLF_BOT_TOKEN both have a token
    When I run werewolf-bot
    Then the token from WEREWOLF_BOT_TOKEN is used

  Scenario: A rejected token gives a clear error
    # not tested: gap
    Given the token is rejected by Telegram
    When I run werewolf-bot
    Then the bot prints that Telegram rejected the token
    And it exits with a non-zero code without creating a game or touching the database

  Scenario: A config file can be passed explicitly
    # covered by: relative_paths_resolve_from_the_config_folder
    When I run werewolf-bot --config /srv/wolf/werewolf.toml
    Then that file is used and relative paths in it resolve from its folder

  Scenario: The database is created on first start
    # covered by: the_database_file_is_created_with_the_current_schema, sqlite_urls_get_create_mode
    Given no database file exists at the configured path (default werewolf.db)
    When the bot starts
    Then the database file is created with the current schema
    And no database server is needed

  Scenario: A Postgres URL uses Postgres
    # covered by: group_settings_persist_on_both_backends
    Given werewolf.toml sets database = "postgres://wolf@db/werewolf"
    When the bot starts
    Then it connects to that Postgres database and applies pending migrations

  Scenario: Beta and main keep separate databases
    # not tested: deployment practice (one werewolf.toml and database per bot)
    Given the main bot and the beta bot each have their own werewolf.toml
    When the beta bot starts with a newer schema
    Then only the beta database is migrated
    And the main bot keeps running on its own database

  Scenario Outline: Both backends pass the same storage tests
    # covered by: group_settings_persist_on_both_backends
    When CI runs the db test suite against <backend>
    Then every storage scenario in this file passes
    Examples:
      | backend                 |
      | in-memory SQLite        |
      | Postgres in a container |

  Scenario: A newer bot upgrades an older database
    # covered by: a_newer_bot_backs_up_then_migrates_an_older_database, migrating_twice_is_a_no_op
    Given a database created by an older version of the bot
    When a newer version starts
    Then for SQLite a copy is saved as werewolf.db.bak-<old schema version>
    And the pending migrations are applied in order

  Scenario: An older bot refuses a newer database
    # covered by: an_older_bot_refuses_a_newer_database
    Given a database migrated by a newer version of the bot
    When an older version starts
    Then it refuses to start and says the database is newer than the bot

  Scenario: English works with no language files on disk
    # covered by: embedded_english_loads_and_fills
    Given the configured languages folder does not exist
    When the bot starts
    Then English messages are served from the copy built into the binary

  Scenario: Extra languages come from a folder
    # covered by: every_shipped_language_file_loads, loads_all_language_files_without_error
    Given the languages folder holds the community language XML files
    When the bot starts
    Then those languages load in their existing format

  Scenario Outline: One binary per platform
    # not tested: release-build job in .github/workflows/rust.yml, outside cargo xtask ci
    When CI builds a release for <target>
    Then it produces a single werewolf-bot executable that needs no other installed library
    Examples:
      | target                    |
      | x86_64-unknown-linux-gnu  |
      | aarch64-unknown-linux-gnu |
      | x86_64-apple-darwin       |
      | aarch64-apple-darwin      |
      | x86_64-pc-windows-msvc    |

Feature: Shutting the bot down

  Scenario: Ctrl+C stops new games and waits for running ones
    # covered by: shutdown_lets_games_finish_or_stops_them
    Given a game is running in one group
    When the operator presses Ctrl+C once
    Then /startgame and /startchaos are refused with a "bot is restarting" message
    And the running game plays to its end before the bot exits

  Scenario: A second Ctrl+C forces the exit
    # covered by: shutdown_lets_games_finish_or_stops_them
    Given the bot is waiting for running games to finish
    When the operator presses Ctrl+C again
    Then every running game is ended with a message in its group
    And the bot exits

  Scenario: Games cut off by a crash are announced on the next start
    # covered by: games_cut_off_by_a_crash_are_announced_on_restart, games_cut_off_by_a_crash_are_found_and_marked_interrupted
    Given the bot stopped while a game was running in "Village Square"
    When the bot starts again
    Then "Village Square" is told its last game was interrupted
    And that game is recorded as interrupted in the history

Feature: Storage other parts of the bot can persist through
  # The db crate exposes one Store with a repository per area, the same API on
  # both backends. Other crates never write SQL themselves.

  Scenario: Group settings survive a restart
    # covered by: group_settings_survive_a_restart
    Given the group "Village Square" has a night timer of 120
    When the bot restarts
    Then the group's night timer is still 120

  Scenario: A group with no stored settings gets defaults
    # covered by: a_group_with_no_stored_settings_gets_defaults_and_nothing_is_written
    Given the bot has never seen the group "New Hamlet"
    When its settings are read
    Then the defaults defined by the group-config code are returned
    And nothing is written until a setting changes

  Scenario: New settings fields need no migration
    # covered by: new_settings_fields_need_no_migration
    Given a group's settings were saved before a setting was added to the code
    When those settings are read
    Then the new setting has its default value and the stored values are kept

  Scenario: Players are remembered when they start the bot privately
    # covered by: players_are_remembered_when_they_start_the_bot
    When "Ana" sends /start to the bot in a private chat
    Then Ana's Telegram id, name and username are stored
    And Ana is marked as able to receive private messages

  Scenario: A player's name is refreshed when it changes
    # covered by: a_players_name_is_refreshed_and_reachability_kept
    Given "Ana" is stored
    When Ana joins a game as "Ana B"
    Then the stored name is "Ana B"

  Scenario: Bans are stored and expired bans stop counting
    # covered by: expired_bans_stop_counting_but_stay_in_history
    Given a ban for Ana that expired yesterday
    And a ban for Bob with no expiry
    When the moderation code asks who is banned
    Then Bob is banned and Ana is not
    And Ana's expired ban is still in her ban history

  Scenario: Every finished game is recorded
    # covered by: every_finished_game_is_recorded, a_full_game_plays_to_the_end_and_is_recorded
    When a game in "Village Square" ends with the village winning
    Then the history has the group, mode, start and end time and winner
    And each player's starting role, final role and whether they survived
    And each death with its victim, killer if known, method and day

  Scenario: Recorded games can be exported as replay fixtures
    # covered by: recorded_games_export_as_replayable_fixtures
    Given a recorded game
    When it is exported
    Then it has the same shape as the anonymised fixtures sim already replays

Feature: The join phase in Telegram
  # Lobby rules are already in control::lobby. These scenarios cover what the
  # group sees.

  Scenario: Starting a game opens a lobby with a join button
    # covered by: players_who_never_started_the_bot_are_sent_to_it, the_join_timer_reminds_then_gives_up_without_enough_players
    When "Ana" sends /startgame in "Village Square"
    Then the group gets the new-game message with a "Join" button
    And the button opens a private chat with the bot that joins Ana to that game

  Scenario: /startchaos opens a chaos lobby
    # covered by: a_chaos_lobby_reports_chaos_mode, the_game_mode_setting_decides_which_modes_a_game_can_start_in
    When "Ana" sends /startchaos in "Village Square"
    Then the lobby uses chaos mode

  Scenario: Starting a game in a private chat is refused
    # covered by: one_game_per_group_and_none_in_private
    When "Ana" sends /startgame in a private chat with the bot
    Then she is told games can only be started in groups

  Scenario: Only one game per group
    # covered by: one_game_per_group_and_none_in_private
    Given a lobby is open in "Village Square"
    When "Bob" sends /startgame there
    Then Bob is told a game is already being joined and pointed to the join button

  Scenario: Joining needs a private chat with the bot
    # covered by: players_who_never_started_the_bot_are_sent_to_it
    Given "Cal" has never started the bot privately
    When Cal presses "Join"
    Then Cal is asked to start the bot privately first
    And Cal is not added to the lobby

  Scenario: Joining is announced in the group
    # covered by: joins_are_announced_in_batches, joins_are_announced_every_30_seconds
    Given "Ana" has started the bot privately
    When Ana joins the lobby in "Village Square"
    Then the group is told Ana joined and how many players there are

  Scenario: Join errors are explained in the group's language
    # covered by: rejects_joins_past_max_players, a_player_can_only_be_in_one_lobby
    Given the lobby in "Village Square" is full
    When "Dee" tries to join
    Then Dee is told the game is full

  Scenario: The join timer counts down in the group
    # covered by: the_join_timer_reminds_then_gives_up_without_enough_players, joining_lasts_three_minutes_with_reminders
    Given a lobby is open with the group's join time
    Then the group is reminded at 60, 30 and 10 seconds left
    And when the time runs out the game starts or is cancelled for too few players

  Scenario: /extend moves the join timer
    # covered by: extend_flee_and_players_in_the_lobby, extend_reports_the_new_time_and_is_capped_at_five_minutes
    Given "Ana" has joined the lobby
    When Ana sends /extend 30
    Then 30 seconds are added and the group is told the new time left

  Scenario: /flee during joining leaves the lobby
    # covered by: extend_flee_and_players_in_the_lobby
    Given "Ana" has joined the lobby
    When Ana sends /flee
    Then Ana is removed and the group is told

  Scenario: /forcestart starts immediately
    # covered by: forcestart_is_for_group_admins
    Given 5 players have joined
    When a group admin sends /forcestart
    Then the game starts without waiting for the timer

  Scenario: /killgame ends the lobby
    # covered by: killing_during_joining_aborts_the_lobby
    When a group admin sends /killgame during joining
    Then the lobby is closed and the group is told the game was cancelled

  Scenario: /players lists who has joined
    # covered by: extend_flee_and_players_in_the_lobby
    When anyone sends /players during joining
    Then the group gets the list of joined players

  Scenario: No new games when the server is at capacity
    # covered by: busy_bots_refuse_new_games
    Given the capacity monitor refuses new games
    When "Ana" sends /startgame
    Then the group is told the bot is busy and to try again shortly

Feature: Playing a game in Telegram

  Scenario: Everyone learns their role privately
    # covered by: everyone_is_told_their_role_when_the_game_starts, a_full_game_plays_to_the_end_and_is_recorded
    When a game starts
    Then each player gets a private message with their role and its description
    And the group is told the game has started and night is falling

  Scenario: A night question arrives as buttons
    # not tested: Telegram UI layout (the Skip button is only used by the bot test driver)
    Given "Ana" is the Seer and it is night
    Then Ana gets a private message with one button per living player she can pick
    And a "Skip" button

  Scenario: Everyone with a night action is asked at the same time
    # covered by: a_night_asks_every_independent_role_in_one_batch
    Given it is night and the Seer, the wolves and the Guardian Angel are alive
    Then all three questions are sent together
    And they share one night timer from the group's settings

  Scenario: Questions that depend on another answer follow it
    # covered by: the_witch_is_asked_in_a_second_batch_after_the_wolves, witch_is_asked_only_after_the_wolves_target_resolves
    Given a role whose question depends on another role's answer
    Then its question is sent once that answer is in or the timer has run out
    And it gets the time left on the same night timer

  Scenario: Answering records the choice and shows it
    # not tested: gap
    Given Ana has a Seer question open
    When Ana presses "Bob"
    Then Ana's choice is recorded
    And the message is edited to show she chose Bob, without buttons

  Scenario: Not answering in time counts as no answer
    # covered by: nobody_answering_still_ends_the_game, malformed_or_missing_batch_answers_count_as_no_answer
    Given Ana has a question open
    When the night timer runs out
    Then Ana's question counts as no answer
    And the message is edited to say time is up

  Scenario: A late or stale button does nothing
    # covered by: buttons_from_a_finished_game_say_so
    Given Ana's question from last night is still visible
    When Ana presses one of its buttons
    Then she is told that question is closed
    And nothing in the game changes

  Scenario: A two-target question needs two picks
    # not tested: gap
    Given "Ana" is Cupid on the first night
    When Ana presses "Bob" and then "Cal"
    Then both are recorded as her lovers' choice

  Scenario: Yes/no questions use two buttons
    # not tested: superseded by bot-messages.feature (one-tap abilities get a single button)
    Given "Ana" is the Mayor during the day
    Then Ana is asked whether to reveal with "Yes" and "No" buttons

  Scenario: The day has a discussion time before the lynch vote
    # covered by: phase_timers_are_announced_with_the_groups_durations, lynch_votes_are_announced_as_they_are_cast
    When day begins
    Then the group is told it is day and can talk for the group's day timer
    And then every living player gets a private lynch vote with one shared lynch timer

  Scenario: Deaths, transforms and the result are announced in the group
    # covered by: a_full_game_plays_to_the_end_and_is_recorded, fuzzed_games_never_leak_placeholders_or_miscount_deaths
    When the engine reports a death, a transform or the end of the game
    Then the group gets the matching message in its language

  Scenario: An event with no translation yet still gets announced
    # covered by: fuzzed_games_never_leak_placeholders_or_miscount_deaths, missing_keys_use_the_fallback, fallback_texts_cover_only_keys_missing_from_english_xml
    Given an event whose locale key is not ported yet
    When it happens
    Then the group gets the plain English line the transcript narrator already writes
    And no raw key or {0} placeholder is ever sent

  Scenario: Fleeing during a game takes effect at the next checkpoint
    # covered by: a_fled_player_dies_by_flee_and_is_removed, fleeing_mid_game_can_immediately_hand_the_wolves_a_win
    Given a game is running and "Ana" is alive
    When Ana sends /flee
    Then Ana is told she will leave at the next phase change
    And at the next checkpoint she is removed as fled

  Scenario: /killgame ends a running game
    # covered by: killgame_ends_a_running_game, an_admin_killing_the_game_ends_it_with_no_winner
    When a group admin sends /killgame during a game
    Then the game ends at the next checkpoint with a message in the group

  Scenario: /players during a game shows the living and the dead
    # covered by: the_group_sees_the_player_list_each_phase_and_the_game_length, a_dead_player_with_reveal_shows_role_and_love_status
    When anyone sends /players during a game
    Then the group gets the in-game player list

  Scenario: A player who blocked the bot is treated as not answering
    # covered by: a_player_who_blocks_the_bot_mid_game_does_not_stop_it
    Given "Ana" has blocked the bot
    When Ana is asked a question
    Then the question counts as no answer and the game carries on

  Scenario: Telegram rate limits are waited out
    # covered by: flood_control_is_waited_out, rate_limits_are_waited_out
    When Telegram answers a send with "retry after 5 seconds"
    Then the bot waits 5 seconds and sends the message again

Feature: Batched questions in the engine

  Scenario: Existing presenters behave exactly as before
    # covered by: transcripts_match_the_committed_digests, all_deterministic_fixtures_match
    Given a presenter that does not implement the batched ask
    When run_game runs
    Then the questions are asked in the same order as today
    And the full existing test suite passes unchanged

  Scenario: A batched presenter gets each phase's independent questions at once
    # covered by: a_night_asks_every_independent_role_in_one_batch
    Given a presenter that records batches
    When a night with a Seer, two Wolves and a Guardian Angel runs
    Then the first batch holds all four questions

Feature: Private-chat basics

  Scenario: /start registers a player
    # covered by: players_are_remembered_when_they_start_the_bot, flood_control_is_waited_out
    When "Ana" sends /start privately
    Then she gets the welcome message and is stored as reachable

  Scenario: /ping answers
    # covered by: flood_control_is_waited_out
    When anyone sends /ping
    Then the bot answers with how long it took to reply
