# Scope for porting /setlang and the group language picker
# (approval delegated to the coordinator, 10 Oct 2026)
#
# Legacy sources:
#   GeneralCommands.cs:112-185      /setlang
#   UpdateHandler.cs:1408-1500      "lang" (group config button) and "setlang" callbacks
#   UpdateHandler.cs:2071-2105      SelectLanguage: base list, variant list
#   Helpers/LanguageHelper.cs:23-46 LangFile: name, base, variant, isDefault from <language>
#   Commands/Helpers.cs:270-304     GetLanguage: group, then player, then English
#   Database/GroupConfig.cs         RandomLangVariant
#
# Proposed home: `control::language_picker`, sharing the Telegram-free menu
# types with `group_config` (moved to a common `control::menu` module, with
# one callback-data codec for both). The group's language is a field of
# GroupSettings (JSON, no migration); a player's goes through
# db players().set_language.
#
# Out of scope:
#   - The "Random" variant. Its button is commented out in the legacy code
#     ("TO DO: Publish Random Language Variant Mode", UpdateHandler.cs:2081),
#     so no user could pick it. RandomLangVariant is not ported.
#   - /getlang (download language files). It reuses the same picker and
#     comes next, together with file sending.
#   - Loading the chosen language into a running game (Werewolf.cs's
#     LoadLanguage); that belongs with the Telegram presenter.

Feature: Choosing a language
  As a player or a group admin
  I want to choose which translation the bot speaks
  So that the game reads in my language or my group's favourite variant

  Background:
    Given the language files in "Werewolf for Telegram/Languages"

  Scenario: The base list shows every base language once, sorted, two per row
    When a player runs /setlang
    Then they are sent "WhatLang" with their current base language, privately
    And the buttons are every distinct base language, sorted, two per row

  Scenario: /setlang in a group answers privately and says so in the group
    When a player runs /setlang in a group
    Then the base list is sent to them privately
    And the group is told "SentPrivate"

  Scenario: A player who never started the bot is asked to
    Given the bot cannot message the player privately
    When they run /setlang in a group
    Then the group is asked to start a private chat with the bot

  Scenario: A player with no language set is treated as English
    Given a player with no language
    When they run /setlang
    Then "WhatLang" shows "English"

  Scenario: Picking a base with one file sets it straight away
    Given the base "Thai" has a single language file
    When the player picks "Thai"
    Then their language is that file
    And they see "LangSet" with "Thai: Normal"

  Scenario: Picking a base with several files asks for the variant
    When the player picks "English"
    Then they see "WhatVariant" with their current variant
    And the buttons are English's variants, the default first, then sorted by name, two per row

  Scenario: Picking a variant sets the language
    When the player picks "English" then the "Star Wars" variant
    Then their language is the "English Star Wars" file
    And they see "LangSet" with "English: Star Wars"

  Scenario: The group's language is a config option
    When an admin opens "GroupSettings" in the config menu
    Then the first option is "Lang"
    When they press it
    Then they see "WhatLang" with the group's current base language

  Scenario: Setting the group's language returns to the config menu
    When an admin picks a language for the group
    Then the group's language is that file
    And they are told "LangSet" with the language
    And they see the main config menu

  Scenario: Non-admins cannot change the group's language
    When a user who is not an admin picks a language for the group
    Then they are shown the "GroupAdminOnly" alert
    And the group's language does not change

  Scenario: A group's language defaults to English
    Then a group that never chose a language uses "English"

  Scenario: Every language button fits in Telegram's callback data
    Then every base and variant button encodes to 64 bytes or fewer
    And decodes back to the same action
