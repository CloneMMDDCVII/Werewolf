# Scope: admin and moderation commands (Rust port)
# Status: DRAFT, awaiting Pierre's approval. No code until approved.
#
# Legacy sources (Werewolf for Telegram/):
#   smite      Werewolf Control/Commands/AdminCommands.cs:26-63
#              -> Models/GameInfo.cs:74-81 -> Werewolf Node/Werewolf.cs:5357-5391 (FleePlayer)
#   getban     AdminCommands.cs:131-158, target lookup Helpers/Extensions.cs:136-185 (GetTarget)
#   permban    DevCommands.cs:848-980      remban  DevCommands.cs:982-1030
#   gating     Handlers/UpdateHandler.cs:330-440 (ban silence, anonymous, global/group admin)
#
# Where it lands:
#   moderation crate  -> bans, ban lookup/formatting, command permission gate, target resolution
#   control::lobby    -> smite while joining
#   game-engine       -> smite in a running game (reuses the existing flee seam, no new death path)
#
# Not in this scope (each gets a COMMANDS.md note saying why):
#   config, setlink, remlink     group settings, owned by the group-config thread
#   getidles                     needs 24h idle-kill queries; waits for the db crate
#   restore                      account merge over game history; waits for the db crate
#   uploadlang, validatelangs    i18n tooling, not moderation
#   addach, remach               achievements are dropped
#   reviewgifs, approvegifs, disapprovegifs, fixgifs   donation gifs, returning later with Stars
#   fi, getbans                  bot infra / spam-list dump (getbans reads the in-memory spam list)
#   Telegram parsing of entities (@mention, text mention, reply) is the presenter's job;
#   the port takes already-resolved targets.

Feature: Command permission gate
  Every command declares who may run it. The gate runs before the command and
  answers either "run it" or the refusal message the legacy bot sent.

  Background:
    Given "Alice" is a global admin
    And "Gary" is an admin of group "Pack"
    And "Paul" is an ordinary player

  Scenario: A banned user is ignored completely
    # covered by: a_banned_user_is_ignored_even_for_open_commands, a_ban_survives_a_restart_and_silences_the_player
    Given "Paul" has an active ban
    When "Paul" sends any command
    Then the command is dropped with no reply
    # UpdateHandler.cs:330-334

  Scenario: An expired ban no longer silences the user
    # covered by: an_expired_ban_no_longer_counts, an_expired_stored_ban_is_not_loaded
    Given "Paul" had a ban that expired yesterday
    When "Paul" sends "/players"
    Then the command runs

  Scenario Outline: Group-admin commands
    # covered by: group_admin_commands_accept_every_kind_of_admin, group_admin_commands_refuse_ordinary_players
    When <user> sends "/smite" in group "Pack"
    Then the gate answers <result>
    Examples:
      | user                   | result                       |
      | "Gary"                 | run                          |
      | "Alice"                | run                          |
      | a dev                  | run                          |
      | an anonymous Pack admin| run                          |
      | "Paul"                 | refuse with "GroupAdminOnly" |

  Scenario: Group-only commands are refused in private chat
    # covered by: group_only_commands_are_refused_in_private
    When "Gary" sends "/smite" in a private chat
    Then the gate refuses with "GroupCommandOnly"

  Scenario: Global-admin commands
    # covered by: global_admin_commands_refuse_group_admins
    When "Gary" sends "/getban Paul"
    Then the gate refuses with "NotGlobalAdmin"

  Scenario: Anonymous senders on a command that does not allow them
    # covered by: anonymous_admins_only_where_allowed
    When an anonymous Pack admin sends "/getban Paul"
    Then the gate refuses with "ExitAnonymousMode"

  Scenario: A channel posting into the group is always refused
    # covered by: channels_are_always_refused
    When a linked channel sends "/smite Paul" in group "Pack"
    Then the gate refuses with "ExitAnonymousMode"

  Scenario: Blockable commands do nothing in support and translation chats
    # covered by: blockable_commands_are_refused_in_support_chats
    When "Gary" sends "/smite Paul" in the support chat
    Then the gate refuses with the support-chat notice

Feature: /smite removes a player from the group's game
  Smite is a forced flee: same outcome as /flee, but chosen by an admin,
  with none of /flee's player-side checks.

  Scenario: Smiting a player while joining removes them from the lobby
    # covered by: smiting_a_joined_player_removes_them, smite_removes_a_player_from_the_lobby
    Given group "Pack" has a lobby with "Paul" and "Rita" joined
    When "Gary" smites "Paul"
    Then "Paul" is no longer in the lobby
    And the group is told "Paul" fled ("Flee")
    And no "players remain" count is sent
    # Werewolf.cs:5384-5389, unlike /flee at 861-865

  Scenario: Smiting a player in a running game kills them as fled
    # covered by: smiting_several_players_kills_them_in_order_and_checks_for_a_win, smiting_the_last_wolf_hands_the_village_the_win
    Given a running game where "Paul" is alive
    When "Gary" smites "Paul"
    Then "Paul" dies with kill method Flee
    And their role is revealed only if the group shows roles on death
    And lover deaths, transforms and win checks run as for any death

  Scenario: Smite ignores the group's "allow flee" setting
    # not tested: gap
    Given a running game in a group that forbids fleeing
    When "Gary" smites "Paul"
    Then "Paul" dies with kill method Flee
    # RemovePlayer checks AllowFlee (Werewolf.cs:835); FleePlayer does not

  Scenario: Smiting a dead player does nothing and says nothing
    # covered by: smiting_the_same_player_twice_kills_them_once
    Given a running game where "Paul" is already dead
    When "Gary" smites "Paul"
    Then nothing changes and no message is sent
    # /flee would answer "DeadFlee"; smite is silent (Werewolf.cs:5367-5370)

  Scenario: Smiting someone not in the game does nothing
    # covered by: smiting_someone_who_never_joined_changes_nothing, smiting_someone_not_in_the_game_changes_nothing
    When "Gary" smites a user who never joined
    Then nothing changes and no message is sent

  Scenario: Smiting several targets at once
    # covered by: smite_collects_reply_mentions_and_id_in_order, smiting_several_players_at_once, smiting_several_players_kills_them_in_order_and_checks_for_a_win
    When "Gary" smites "Paul" and "Rita" in one command
    Then both are removed, in the order given

  Scenario: Smite while the game is starting up is refused
    # not tested: gap
    Given the lobby has closed and roles are being assigned
    When "Gary" smites "Paul"
    Then the group is told "Cannot flee while game is initializing.  Try again once game is done starting."

  Scenario: Smite with no game in the group
    # not tested: gap
    Given group "Pack" has no lobby or game
    When "Gary" smites "Paul"
    Then nothing happens

  Scenario: A smite lands at the next checkpoint, not mid-question
    # covered by: smiting_several_players_kills_them_in_order_and_checks_for_a_win, a_fled_player_dies_by_flee_and_is_removed
    Given a running game is waiting for night answers
    When "Gary" smites "Paul"
    Then "Paul" is removed at the next of the three per-round checkpoints
    # Same documented approximation as /flee and /killgame

Feature: /getban reports a player's ban status

  Scenario: Player with a temporary ban
    # covered by: getban_shows_a_temporary_ban, getban_reads_the_stored_ban_and_first_game
    Given "Paul" was banned by "Alice" on 01OCT2026 for "spam", expiring in 3 days 4 hours 5 minutes
    When "Alice" runs "/getban" on "Paul"
    Then the reply reads:
      """
      Player: Paul
      Current Status: <b>Banned for: spam</b>
      By: Alice on 01OCT2026 12:00:00 +00:00
      Ban expiration: <b>3 days, 4 hours, 5 minutes</b>
      Player first seen: <first game start, or "Hasn't played ever!">
      """

  Scenario: A ban expiring more than a year away is shown as permanent
    # covered by: getban_shows_far_off_and_permanent_bans_as_perm
    Given "Paul" has a ban expiring in 400 days
    When "Alice" runs "/getban" on "Paul"
    Then the status ends with "<b>Perm Ban</b>"

  Scenario: Player with no ban
    # covered by: getban_without_a_ban
    When "Alice" runs "/getban" on "Rita"
    Then the status is "Not banned (in Werewolf)"

  Scenario: Player who never played
    # covered by: getban_shows_a_temporary_ban
    Given "Rita" has no game history
    Then "Player first seen" is "Hasn't played ever!"

  Scenario: Player names are HTML-escaped
    # covered by: getban_escapes_names_and_reasons
    Given a player named "<b>x</b>"
    Then the reply shows "&lt;b&gt;x&lt;/b&gt;"

  Scenario Outline: Choosing the target
    # covered by: a_reply_targets_its_author, a_reply_to_a_forward_targets_the_original_author, a_username_mention_is_case_insensitive, a_numeric_id_targets_that_player, no_argument_falls_back_to_the_sender, an_unknown_target_falls_back_to_the_sender
    When "Alice" runs "/getban" <how>
    Then the target is <target>
    Examples:
      | how                                    | target                     |
      | replying to a message from "Paul"      | "Paul"                     |
      | replying to a message forwarded from "Paul" | "Paul"                |
      | with "@paul" (any case)                | "Paul"                     |
      | with Paul's numeric id                 | "Paul"                     |
      | with no argument                       | "Alice" herself            |
      | with an unknown name                   | "Alice" herself            |
    # GetTarget falls back to the sender, a legacy quirk kept for parity

  # Deliberate change, flagged for approval: legacy getban shows a ban that
  # has already expired (negative time) until the next ban-list refresh.
  # The port treats an expired ban as "Not banned".

Feature: Issuing and lifting bans (permban / remban)
  Ported as data operations on the moderation crate, without redesigning
  how bans are issued. Only included if Pierre picks that option.

  Scenario: Permanently banning a player by id
    # covered by: permban_by_id_bans_permanently_and_smites, permban_of_a_never_seen_id_creates_the_player
    When "Alice" bans id 123 for "cheating"
    Then a Native ban exists for 123 with reason "cheating", issued by "Alice", no expiry
    And if 123 is in this group's game they are smitten
    And the reply is "User Unknown User (@) has been banned" when 123 has never played

  Scenario: Banning by mention
    # covered by: permban_by_mention
    When "Alice" bans "@paul" for "cheating"
    Then the reply is "User has been banned"

  Scenario: Unknown username
    # covered by: permban_of_an_unknown_username_stores_nothing
    When "Alice" bans "@nobody"
    Then the reply is "Unknown user: @nobody" and nothing is stored

  Scenario: Lifting a ban
    # covered by: remban_lifts_by_mention_and_id, remban_lifts_but_keeps_history
    Given "Paul" has a ban
    When "Alice" runs "/remban" on "Paul"
    Then the ban is removed and the reply is "User has been unbanned."

  Scenario: Lifting a ban that does not exist
    # covered by: remban_without_a_ban_says_nothing
    When "Alice" runs "/remban" on "Rita"
    Then nothing is sent

  Scenario: Migrated legacy bans behave the same as new ones
    # covered by: migrated_bans_are_lifted_the_same_way, migrated_max_value_bans_become_permanent, migrated_temporary_bans_keep_their_expiry
    Given a ban migrated from the legacy GlobalBan table
    Then it silences the user, shows in /getban and can be lifted with /remban

# ---------------------------------------------------------------------------
# Addendum (10 Oct 2026): getidles and restore, on the db storage layer.
# Directed by the coordinator under Pierre's delegated approval.
# Sources: AdminCommands.cs:220-271 (getidles), werewolf.sql:909-960
# (Get[Group]IdleKills24Hours), AdminCommands.cs:496-581 (restore),
# UpdateHandler.cs:1125-1152 and werewolf.sql:1377-1413 (RestoreAccount).
# Bans, players and games persist through the db crate; no new tables.

Feature: /getidles counts a player's idle kills in the last 24 hours
  Group admins (anonymous allowed), any chat, not blockable.

  Scenario: One player by id
    # covered by: getidles_counts_the_last_24_hours
    Given "Paul" (id 2) was killed for idling 3 times in 24 hours, 1 of them in this group
    When "Gary" sends "/getidles 2" in group "Pack"
    Then the reply is "Player 2 (Paul) has been killed for not voting 3 times in the past 24 hours. 1 of those kills were in this group."

  Scenario: Kills older than 24 hours do not count
    # covered by: getidles_counts_the_last_24_hours
    Given "Paul" idled 25 hours ago
    Then his count is 0

  Scenario: Several targets
    # covered by: targets_come_from_ids_then_text_mentions_then_the_reply
    When "Gary" sends "/getidles 2 3" replying to a message from "Rita"
    Then there is one line per id, then text mentions, then the replied-to author

Feature: /restore moves an old account's history to a new one
  Global admins check; a dev confirms.

  Scenario: Bad input
    # covered by: parses_two_ids_or_asks_for_usage, restore_checks_then_moves_the_history
    When "Alice" sends "/restore 5"
    Then the reply is "usage: /restore <oldid> <newid>"

  Scenario: Unknown account
    # covered by: restore_checks_then_moves_the_history
    When either id has no player record
    Then the reply is "Account not found in database"

  Scenario: Old account is banned
    # covered by: a_banned_old_account_stops_the_check, restore_refuses_a_banned_old_account
    Given the old account has a ban in force
    Then the reply is "Old account was global banned!"

  Scenario: Likeness report
    # covered by: a_clean_move_scores_100, every_warning_costs_its_points
    When both accounts exist
    Then the reply lists every warning (old id newer, games overlap,
      share of the new account's groups seen on the old one, name and
      username edit distance, different languages)
    And ends with "Accuracy score: <n>%" and asks for confirmation
    # 100, minus 30 / 30 / up to 20 / half the summed distances / 10

  Scenario: Confirming the restore
    # covered by: restore_checks_then_moves_the_history
    When a dev confirms
    Then every game and kill of the old account belongs to the new one
    And the reply is "Restored stats from <old name> to <new name>"
