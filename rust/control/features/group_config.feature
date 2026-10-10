# Scope for porting the group `config` command (approved 10 Oct 2026)
#
# Legacy sources:
#   AdminCommands.cs:66-100            /config entry point
#   UpdateHandler.cs:1039-1055         back / cancel / submenu routing
#   UpdateHandler.cs:1540-1985         per-option handlers, role toggles, validate
#   UpdateHandler.cs:2169-2300         main menu, submenus, role menu builders
#   Database/GroupConfig.cs            boolean settings, defaults, menu grouping
#   Database/Group.cs                  stored columns (timers, mode, max players...)
#   Shared/GameBalancing.cs:34-39,135  disabled roles only apply once VALID
#   Werewolf.cs:150-216,816,835,...    where the node reads each setting
#
# Proposed home: a new `control::group_config` module. It holds a typed
# `GroupSettings` struct (no i64 bitflags, same choice balancing.rs made for
# IRole) and a Telegram-free menu state machine: callbacks in, "screen"
# intents out (a locale key, its arguments, and typed buttons). Persistence is
# a `GroupSettingsStore` trait with an in-memory implementation, since `db` is
# still empty.
#
# Out of scope, listed so nothing is dropped silently:
#   - Language picker (lang/setlang, RandomLangVariant). It shares
#     SelectLanguage with the per-user /setlang, so port them together.
#   - Decoding legacy Flags/RoleFlags columns and the `Update` migration
#     flag (UpdateFlags). That belongs with the DB migration.
#   - Engine behaviour of SecretLynch*, ShowRolesDeath, ThiefFull, ShowIDs,
#     ShufflePlayerList, RandomMode, AllowNSFW. Settings are stored and
#     editable, but the mechanics that read them are unported. COMMANDS.md
#     will say so.
#   - AllowTanner/AllowFool/AllowCult/AllowThief/AllowArsonist. They are
#     tagged RoleConfig, but the RoleConfig submenu shows per-role toggles
#     instead (UpdateHandler.cs:2199), so no admin can reach them, and the
#     node never reads them for balancing (AllowArsonist is assigned at
#     Werewolf.cs:211 and never used). Per-role toggles replace them.
#   - Telegram rendering, PM delivery, and checking real chat admin status.
#     The caller says whether the user is an admin.

Feature: Group configuration
  As a group admin
  I want to change how Werewolf games run in my group
  So that games match my group's preferences

  Background:
    Given a group "Village Square" with id -100123

  # ---------- Defaults and the settings model ----------

  Scenario: A group that has never been configured gets the legacy defaults
    # covered by: a_group_that_has_never_been_configured_gets_the_legacy_defaults
    When an admin runs /config in the group for the first time
    Then the group's settings are created with:
      | setting                 | value  |
      | mode                    | Player |
      | max players             | 35     |
      | day timer               | 60     |
      | night timer             | 90     |
      | lynch timer             | 90     |
      | max extend              | 60     |
      | show roles at end       | All    |
      | allow flee              | yes    |
      | show roles on death     | yes    |
      | allow extend            | no     |
      | secret lynch            | no     |
      | secret lynch show votes | no     |
      | secret lynch voters     | no     |
      | random mode             | no     |
      | thief full              | no     |
      | show ids                | no     |
      | allow nsfw              | no     |
      | shuffle player list     | no     |
      | burning overkill        | no     |
      | disabled roles          | none   |
      | roles validated         | no     |
    And the group's name is stored as "Village Square"

  Scenario: Running /config again refreshes the group's name but keeps its settings
    # covered by: running_config_again_refreshes_the_groups_name_but_keeps_its_settings
    Given the group has max players set to 20
    And the group has been renamed to "Village Square II"
    When an admin runs /config in the group
    Then the group's name is stored as "Village Square II"
    And the group's max players is still 20

  # ---------- Who can use it ----------

  Scenario: /config only works inside a group
    # covered by: config_only_works_inside_a_group
    When a user runs /config in a private chat
    Then the command is rejected as group-only

  Scenario: Non-admins cannot open the config menu
    # covered by: non_admins_cannot_open_the_config_menu
    When a user who is not a group admin runs /config in the group
    Then the command is rejected as admin-only
    And no settings are created

  # UpdateHandler.cs:1026-1036 answers every config press from a non-admin
  # with a GroupAdminOnly alert before any option handler runs, so the
  # per-option "remove the buttons" checks further down are unreachable.
  Scenario: A non-admin pressing a config button is told only admins can
    # covered by: a_non_admin_pressing_a_config_button_is_told_only_admins_can
    Given an admin has opened the config menu
    When a user who is not a group admin presses a config button
    Then they are shown the "GroupAdminOnly" alert
    And no setting changes

  Scenario: Global admins can configure any group
    # covered by: global_admins_can_configure_any_group
    When a global admin who is not a group admin presses "Timers"
    Then they see the Timers submenu

  # ---------- Menu structure ----------

  Scenario: The main menu is sent privately to the admin
    # covered by: the_main_menu_is_sent_privately_to_the_admin
    When an admin runs /config in the group
    Then the admin is sent the "WhatToDo" prompt privately
    And the menu shows, two per row:
      | Timers | RoleConfig    |
      | Mechanics | GroupSettings |
      | Done   |               |

  Scenario: If the bot cannot message the admin privately, the group is asked to start a PM
    # covered by: if_the_bot_cannot_message_the_admin_privately_the_group_is_asked_to_start_a_pm
    Given the admin has never started a private chat with the bot
    When the admin runs /config in the group
    Then the group is asked to start a private chat with the bot

  Scenario Outline: Each submenu lists its options followed by Back
    # covered by: each_submenu_lists_its_options_followed_by_back
    When an admin opens the "<submenu>" submenu
    Then the options shown are "<options>"
    And the last button is "Back", which returns to the main menu

    Examples:
      | submenu       | options                                                                           |
      | Timers        | MaxExtend, DayTimer, LynchTimer, NightTimer                                       |
      | Mechanics     | EndRoles, EnableSecretLynch, RandomMode, ShowRolesDeath, ThiefFull, SecretLynchShowVotes, SecretLynchShowVoters |
      | GroupSettings | Mode, MaxPlayer, AllowExtend, AllowFlee, ShowIDs, AllowNSFW, ShufflePlayerList, BurningOverkill |

  Scenario: Done closes the menu
    # covered by: done_closes_the_menu
    When an admin presses "Done"
    Then they see "ThankYou" with no buttons

  Scenario: Cancel on any option returns to that option's submenu without changing anything
    # covered by: cancel_on_any_option_returns_to_that_options_submenu_without_changing_anything
    Given the group's night timer is 90
    When an admin opens "NightTimer" and presses "Cancel"
    Then they see the Timers submenu
    And the night timer is still 90

  # ---------- Yes/no settings ----------

  Scenario Outline: A yes/no setting shows its current value and its own wording
    # covered by: a_yes_no_setting_shows_its_current_value_and_its_own_wording
    Given the group's "<setting>" is <current>
    When an admin opens "<setting>"
    Then they see "<setting>Q" with "<current label>"
    And the choices are "<yes>", "<no>" and "Cancel"

    Examples:
      | setting           | current | current label | yes   | no       |
      | AllowFlee         | on      | Allow         | Allow | Disallow |
      | EnableSecretLynch | off     | No            | Yes   | No       |
      | ShowRolesDeath    | on      | Show          | Show  | Hide     |
      | BurningOverkill   | off     | Disallow      | Allow | Disallow |

  Scenario: Changing a yes/no setting confirms it and returns to its submenu
    # covered by: changing_a_yes_no_setting_confirms_it_and_returns_to_its_submenu
    When an admin sets "EnableSecretLynch" to "Yes"
    Then the group's secret lynch is on
    And the admin is told "EnableSecretLynchA" with "Yes"
    And they see the Mechanics submenu

  # ---------- Choice settings ----------

  Scenario Outline: Choice settings offer the legacy values
    # covered by: choice_settings_offer_the_legacy_values, every_choice_button_stores_its_value_and_returns_to_its_submenu
    When an admin opens "<option>"
    Then the choices are "<choices>" and "Cancel"

    Examples:
      | option     | choices                       |
      | Mode       | Normal, Chaos, Player         |
      | EndRoles   | None, Living, All             |
      | MaxPlayer  | 5, 10, 15, 20, 25, 30, 35     |
      | MaxExtend  | 60, 120, 180, 240, 300        |
      | NightTimer | 30, 60, 90, 120               |
      | LynchTimer | 30, 60, 90, 120               |

  Scenario: The day timer keeps the legacy 60-second offset between stored and shown values
    # covered by: the_day_timer_keeps_the_legacy_60_second_offset_between_stored_and_shown_values
    When an admin opens "DayTimer"
    Then the buttons read "90, 120, 150, 180"
    And pressing "150" stores a day timer of 90
    And the admin is told "SetDayTimeA" with "150"

  Scenario: The game mode setting decides which modes a game can start in
    # covered by: the_game_mode_setting_decides_which_modes_a_game_can_start_in
    Given the group's mode is "Normal"
    Then a lobby built from the group's settings only allows a normal game
    When the mode is "Player"
    Then a lobby built from the group's settings allows whichever mode the player chose

  # ---------- Role configuration ----------

  Scenario: The role menu shows every role that can be disabled, three per row
    # covered by: the_role_menu_shows_every_role_that_can_be_disabled_three_per_row
    When an admin opens "RoleConfig"
    Then they see "RoleConfigInfo"
    And every legacy role except Wolf, Villager and Spumpkin (40 roles) has a toggle with its emoji and ✅ or ❌
    And the menu ends with "EnableAllRoles", "DisableAllRoles", then "Validate" and "Back"

  Scenario: Toggling a role disables it and clears validation
    # covered by: toggling_a_role_disables_it_and_clears_validation
    Given the group's role configuration is validated
    When an admin toggles the Seer
    Then the Seer is disabled
    And the role configuration is no longer validated

  Scenario: Disabling all roles leaves only the roles that cannot be disabled
    # covered by: disabling_all_roles_leaves_only_the_roles_that_cannot_be_disabled
    When an admin presses "DisableAllRoles"
    Then every role except Wolf, Villager and Spumpkin is disabled
    And the role configuration is not validated

  Scenario: Enabling all roles is automatically valid
    # covered by: enabling_all_roles_is_automatically_valid
    When an admin presses "EnableAllRoles"
    Then no roles are disabled
    And the role configuration is validated

  Scenario: Validation passes when every player count from 5 to max players can be balanced
    # covered by: validation_passes_when_every_player_count_from_5_to_max_players_can_be_balanced
    Given the group's max players is 15
    And the Cultist and Serial Killer are disabled
    When an admin presses "Validate"
    Then the role configuration is validated
    And the admin is told "RoleConfigValid"
    And the "Validate" button now reads "Valid"

  Scenario: Validation fails when some player count cannot be balanced
    # covered by: validation_fails_when_some_player_count_cannot_be_balanced
    Given every disableable role is disabled
    When an admin presses "Validate"
    Then the role configuration is not validated
    And the admin is told "RoleConfigInvalid"

  Scenario: Disabled roles only affect games once validated
    # covered by: disabled_roles_only_affect_games_once_validated
    Given the Seer is disabled but the role configuration is not validated
    Then a lobby built from the group's settings disables no roles
    When the role configuration is validated
    Then a lobby built from the group's settings disables the Seer

  Scenario: Raising max players clears validation, lowering it does not
    # covered by: raising_max_players_clears_validation_lowering_it_does_not
    Given the group's max players is 20 and its role configuration is validated
    When an admin sets max players to 25
    Then the role configuration is no longer validated
    Given the role configuration is validated again
    When an admin sets max players to 10
    Then the role configuration is still validated

  # ---------- Settings that reach the game today ----------

  Scenario: Settings the ported lobby already understands come from the group
    # covered by: settings_the_ported_lobby_already_understands_come_from_the_group
    Given the group has max players 20, max extend 120, burning overkill on and mode "Chaos"
    Then a lobby built from the group's settings has max players 20
    And /extend is clamped to 120 seconds
    And role balancing runs in chaos mode with burning overkill

  Scenario: Disallowing extend blocks /extend for non-admins
    # covered by: disallowing_extend_blocks_extend_for_non_admins
    Given the group's allow extend is off
    When a player who is not an admin runs /extend
    Then the request is rejected

  Scenario: Disallowing flee blocks /flee once the game is running
    # covered by: disallowing_flee_blocks_flee_once_the_game_is_running
    Given the group's allow flee is off
    And a game is running
    When a player runs /flee
    Then the request is rejected

  # ---------- Group link (/setlink, /remlink) ----------
  # AdminCommands.cs:273-322. Replies are the legacy hardcoded English.
  # The legacy commands never saved a group they had just created, so a
  # link set before /config was lost; here the group is recorded.

  Scenario: An admin sets the group's invite link
    # covered by: an_admin_sets_the_groups_invite_link
    When an admin runs "/setlink https://t.me/+AbC_12-x" in the group
    Then the group's link is "https://t.me/+AbC_12-x"
    And the group is told "Link set:" with the group's name linked

  Scenario Outline: Only Telegram join links are accepted
    # covered by: only_telegram_join_links_are_accepted
    When an admin runs "/setlink <link>" in the group
    Then the link is <result>

    Examples:
      | link                              | result   |
      | t.me/joinchat/AbC123              | accepted |
      | http://telegram.me/+abc           | accepted |
      | https://t.me/somegroup            | rejected |
      | https://example.com/+abc          | rejected |
      | https://t.me/+abc?start=1         | rejected |

  Scenario: /setlink without a link explains how to use it
    # covered by: setlink_without_a_link_explains_how_to_use_it
    When an admin runs "/setlink" in the group
    Then the group is told "You must use /setlink with the link to the group (invite link)"

  Scenario: A public group already has a link
    # covered by: a_public_group_already_has_a_link
    Given the group has the public username "villagesquare"
    When an admin runs "/setlink https://t.me/+abc" in the group
    Then the group is told "Your group link has already been set to https://telegram.me/villagesquare"
    And no link is stored

  Scenario: An admin removes the group's link
    # covered by: an_admin_removes_the_groups_link
    Given the group's link is "https://t.me/+abc"
    When an admin runs /remlink in the group
    Then the group has no link
    And the group is told "Your group link has been removed."

  Scenario: Only admins can change the link, and only in a group
    # covered by: only_admins_can_change_the_link_and_only_in_a_group
    When a user who is not an admin runs /setlink or /remlink
    Then the command is rejected as admin-only
    When anyone runs them in a private chat
    Then the command is rejected as group-only

  Scenario: The "you joined" message links the group's name
    # covered by: the_you_joined_message_links_the_groups_name
    Given the group's link is "https://t.me/+abc"
    Then the group's name is shown as a link to "https://t.me/+abc", HTML-escaped
