Feature: The bot says what the C# bot said
  The Telegram bot's remaining gaps against the legacy messages
  (notes/ui-audit.md, "Telegram thread (mine)"). Night and role texts
  belong to finish-game-mechanics and are not in this scope.

  Background:
    Given a group with the default settings in English

  # Role PMs with the team (GetRoleInfo) are the engine's:
  # features/role-info.feature (game_engine::role_info).

  Rule: Night prompts name the teammates to agree with (Werewolf.cs:5190-5214)

    Scenario: A wolf is asked to eat with its pack named
      Given Alice and Bob are alive wolves
      When the night asks Alice who to eat
      Then the prompt is AskEat followed by DiscussWith naming Bob

    Scenario: A cultist is asked to convert with the other cultists named
      Given Carol and Dave are alive cultists
      When the night asks Carol who to convert
      Then the prompt is AskConvert followed by DiscussWith naming Dave

  Rule: One-tap abilities get the C#'s single button (Werewolf.cs:2287, 2294, 5327)

    Scenario Outline: The ability's button
      When <role> is asked whether to use their ability
      Then the question has one button labelled <key> and no "No" button
      And not pressing it before the deadline means not using it

      Examples:
        | role      | key    |
        | Mayor     | Reveal |
        | Pacifist  | Peace  |
        | Arsonist  | Spark  |

  Rule: The group sees the player list as each phase starts (Werewolf.cs:1352-1404)

    Scenario Outline: The list is posted when a phase starts
      When the <phase> starts
      Then the group gets the player list after the phase's timer line

      Examples:
        | phase |
        | night |
        | day   |
        | vote  |

    Scenario: An unchanged list is not posted again
      Given nobody died since the last list
      And "Shuffle player list" is off
      When the next phase starts
      Then no new player list is posted

    Scenario: /players during a game points at the latest list
      Given the player list was posted in this game
      When a player sends /players
      Then the bot replies LatestList as a reply to that list

    Scenario: /players before any list was posted
      Given a game has started and no list has been posted
      When a player sends /players
      Then the bot sends the player list

    Scenario: Shuffled lists
      Given the group has "Shuffle player list" on
      When the player list is posted
      Then the dead come first in order of death, then the living in a random order
      And target buttons in every question are in a random order

    Scenario: Show IDs
      Given the group has "Show IDs" on
      When Alice and Bob join and the join batch is announced
      Then HaveJoined lists each name with "(ID: <their id>)" on its own line

  # The game length in the end summary (EndTime) is
  # finish-game-mechanics after-bot/0004's.

  Rule: Remaining hardcoded English goes through the language files

    Scenario: Joining through the PM link without being in the group
      Given Alice has left the group
      When Alice opens the bot's join link for that group
      Then Alice gets NotMember naming the group
      And she is not added to the game

    Scenario: The bot is too busy to start a game
      Given the bot is at its game limit
      When someone sends /startgame
      Then the group gets NoNodes

    Scenario: A game cut off by a crash
      Given the bot stopped mid-game without shutting down
      When the bot starts again
      Then that group gets NodeShutsDown

  Rule: Language admins can check and install language files from Telegram

    Scenario: /validatelangs
      Given Alice is a language admin
      When Alice sends /validatelangs in a private chat
      Then she gets the language-admin menu: a base, then a variant or "All"
      And a variant replies with that file's report, "All" with the base's summary
      # The checks themselves are notes/scopes/language-admin.feature.

    Scenario: /uploadlang
      Given Alice is a language admin
      When Alice replies /uploadlang to a .xml language file
      Then she gets the upload check with buttons to install it or keep the old file
      And pressing "New" installs it and reloads the bot's languages without a restart

    Scenario: /uploadlang without a file
      When Alice sends /uploadlang not as a reply to a .xml document
      Then she is told why it was refused

  Rule: /getlang offers every file at once

    Scenario: All languages
      When someone picks "All" in the /getlang menu
      Then they get one zip of every installed language file
