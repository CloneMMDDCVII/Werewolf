# Scope: finish the unported game-engine mechanics
# Source of truth: Werewolf for Telegram/Werewolf Node/Werewolf.cs (line refs below)
# Delivery: game-engine changes + unit/scripted-presenter tests, new English.xml keys
# claimed in all_mapped_locale_keys(), COMMANDS.md updated. Telegram, DB and group
# config stay out of scope.
#
# Engine change shared by several features: private results to one player
# (Seer reading, freeze notice, Augur hint...) go through a new
# NarrationEvent::Private { to, note } whose `note` enum maps 1:1 to an
# English.xml key, so the narration fuzz test also guards these for {0} leaks.

Feature: Snow Wolf freeze (Werewolf.cs:3092-3184, 5273-5277, 5335-5342)

  Background:
    Given a game with a living Snow Wolf

  Scenario: The Snow Wolf is asked to freeze a non-wolf
    When night begins and Blacksmith silver was not spread today
    Then the Snow Wolf is asked "AskFreeze"
    And the options exclude every wolf-team role and the player frozen last night

  Scenario: Silver spread means no freeze tonight
    Given the Blacksmith spread silver today
    Then the Snow Wolf is not asked to freeze

  Scenario Outline: A frozen player's action fails that same night
    Given the Snow Wolf freezes a <role>
    Then the <role>'s night action tonight has no effect
    And the <role> is told "<key>"
    And the Snow Wolf is told "SuccessfulFreeze"
    Examples:
      | role           | key           |
      | Harlot         | HarlotFrozen  |
      | Chemist        | ChemistFrozen |
      | Cultist        | CultistFrozen |
      | CultistHunter  | CHFrozen      |
      | Seer           | SeeingFrozen  |
      | Sorcerer       | SeeingFrozen  |
      | Fool           | SeeingFrozen  |
      | Oracle         | SeeingFrozen  |
      | Augur          | SeeingFrozen  |
      | GuardianAngel  | GAFrozen      |
      | Villager       | DefaultFrozen |

  Scenario: A frozen Cultist does not vote in the conversion
    Given two Cultists and one is frozen
    Then only the unfrozen Cultist's choice counts (Werewolf.cs:3594)

  Scenario: A frozen Grave Digger's graves are not dug
    Given the Grave Digger dug graves and is frozen
    Then no grave-fall deaths happen tonight and the digger is told "GraveDiggerFrozen"

  Scenario: The Serial Killer is frozen and does not kill
    Given the Snow Wolf freezes the Serial Killer
    Then the Serial Killer is told "SKFrozen" and makes no kill tonight

  Scenario: The Arsonist ignores the freeze
    Given the Snow Wolf freezes the Arsonist
    Then the Arsonist acts normally and is told "ArsonistNotFrozen"
    And the Arsonist cannot be frozen again the next night

  Scenario: The Guardian Angel blocks the freeze
    Given the Guardian Angel protects the Snow Wolf's target
    Then the target is not frozen and the Snow Wolf is told "GuardBlockedSnowWolf"

  Scenario Outline: Freezing a Hunter is a coin flip (RngSource)
    Given the Snow Wolf freezes the Hunter and the roll is <roll>
    Then <outcome>
    Examples:
      | roll  | outcome                                                   |
      | < 50  | the Hunter is frozen and told "DefaultFrozen"             |
      | >= 50 | the Snow Wolf dies by HunterShot and the Hunter is free   |

  Scenario: Freeze wears off
    Given a player was frozen last night
    Then they act normally tonight but cannot be the Snow Wolf's target tonight

  Scenario: The last wolf standing turns the Snow Wolf into a Wolf (Werewolf.cs:4487-4496)
    Given every other wolf-team player is dead
    Then the Snow Wolf transforms into a Wolf and is told "SnowWolfTurnWolf"

Feature: Seer-family result messages (Werewolf.cs:3922-4062)

  Scenario Outline: The Seer sees a role, with legacy disguises
    Given the Seer checks a <target>
    Then the Seer is told "SeerSees" naming the target and role <seen>
    Examples:
      | target    | seen                                   |
      | Villager  | Villager                               |
      | WolfCub   | Wolf                                   |
      | AlphaWolf | Wolf                                   |
      | WolfMan   | Wolf                                   |
      | Lycan     | Villager                               |
      | Traitor   | Wolf or Villager (50/50 via RngSource) |

  Scenario Outline: The Sorcerer only recognises wolves and seers
    Given the Sorcerer checks a <target>
    Then the Sorcerer is told "<key>"
    Examples:
      | target   | key                    |
      | WolfCub  | SeerSees (Wolf)        |
      | Seer     | SeerSees (Seer)        |
      | SnowWolf | SeerSees (SnowWolf)    |
      | Lycan    | SorcererOther          |

  Scenario: The Fool sees a random living role
    Given the Fool checks anyone
    Then the Fool is told "SeerSees" with a random living non-Seer role (wolf types shown as Wolf)

  Scenario: The Oracle sees a role the target is not
    Given the Oracle checks a target
    Then the Oracle is told "NegSeerSees" with a random living role other than the target's
    And if no such role exists the Oracle is told nothing

  Scenario: The Augur sees a role that is not in play
    When night resolves
    Then the Augur is told "AugurSees" with a role not held by anyone alive and not seen before
    And when none remain the Augur is told "AugurSeesNothing"
    And Seer is replaced by Apprentice Seer when the Seer is dead and an Apprentice is alive

  Scenario: The Detective snoops by day, not by night (Werewolf.cs:2933-2953)
    Given the Detective picks a target during the day
    Then after the day the Detective is told "DetectiveSnoop" with the exact role
    And with 40% chance (RngSource) every wolf is told "DetectiveCaught"
    # Correction: the current port asks the Detective at night.

Feature: Troublemaker's double lynch (Werewolf.cs:913-973, 2540-2846)

  Scenario: Trouble causes a second lynch vote the same day
    Given the Troublemaker chooses trouble
    Then the group is told "TroublemakerDoubleLynch"
    And after the first lynch resolves, the group is told "TroubleDoubleLynchNow"
    And a second full lynch vote is held among the survivors

  Scenario: The game can end between the two lynches
    Given the first lynch ends the game
    Then no second vote is held

  Scenario: Whoever acts last wins between Pacifist and Troublemaker
    Given the Pacifist calls peace after the Troublemaker called trouble
    Then there is no lynch today
    # Current port always lets trouble win; C# is order-dependent
    # (917 "peace overrides trouble", 971 "trouble overrides peace").

Feature: Lynch outcomes (Werewolf.cs:2690-2797)
  # Note: legacy has no re-vote on a tie. RandomLynch is hard-coded false, so a
  # tie means nobody dies. The only "re-vote" is Troublemaker's second lynch.

  Scenario: A tied vote kills nobody
    Given two players tie for most votes
    Then nobody is lynched and the group is told "LynchTie"

  Scenario: No votes at all
    Given nobody votes
    Then nobody is lynched and the group is told "NoLynchVotes"

  Scenario: Clumsy Guy's vote may go astray (Werewolf.cs:999-1011)
    Given the Clumsy Guy votes for a player
    Then with 50% chance (RngSource) the vote goes to a random living player other than themselves

Feature: Hunter domino and Gunner bullets

  Scenario: A Hunter shot by a Hunter gets their own final shot (Werewolf.cs:5480-5481)
    Given Hunter A's final shot kills Hunter B
    Then Hunter B is asked for a final shot too, and so on down the chain

  Scenario: A Gunner's bullets are a count, not two flags (Werewolf.cs:1938, 2009, 2479-2486)
    Given a Thief steals a Gunner who has fired once
    Then the Thief becomes a Gunner with one bullet left
    And the Gunner is asked "AskShoot" with the remaining bullet count

# Out of scope here (listed for the parity tracker):
#   Thief full mode (needs group config / chaos-mode flag), Witch (disabled),
#   secret lynch vote display, achievements, Telegram presenter, database.
