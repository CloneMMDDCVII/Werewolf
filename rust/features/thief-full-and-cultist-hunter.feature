# Scope: Thief full mode and Cultist Hunter results (approved by delegation).
# Legacy: Werewolf.cs:1977, 2212-2264 (StealRole), 2349, 2353-2376,
# 4132-4190 (Thief Night), 3556-3590 (Cult Hunter Night), 5290-5330 (menus).

Feature: Thief full mode

  Background:
    Given the group plays with ThiefFull on

  Scenario: The Thief may try to steal every night, and may skip
    When night falls
    Then the Thief is asked "AskThief" with a Skip option, every night

  Scenario: A successful steal swaps roles and the victim becomes the Thief
    Given the Thief visits a Seer and the 50% roll succeeds
    Then the Thief becomes the Seer and the Seer becomes the Thief
    # Werewolf.cs:1977 only turns the victim into a Villager outside full mode.

  Scenario Outline: Steals that fail
    Given the Thief visits <target>
    Then <outcome>
    Examples:
      | target                         | outcome                                        |
      | a Seer, and the roll fails     | the Thief is told "ThiefStealFailed"           |
      | a wolf, cultist or snow wolf   | the Thief is told "ThiefStealFailed"           |
      | a Harlot or GA who went out    | the Thief is told "ThiefStealFailed"           |
      | the Serial Killer              | the Thief dies and is told "StealKiller"       |
      | someone who died tonight       | the Thief is told "ThiefStealDead"             |
      | a Grave Digger who dug, and the Thief falls | the Thief dies, told "ThiefFell"; the digger is told "ThiefFellDigger" |

  Scenario: A frozen Thief cannot steal in full mode
    Given the Snow Wolf froze the Thief
    Then no steal happens and the Thief was told "ThiefFrozen"

Feature: Cultist Hunter results

  Scenario: The Cultist Hunter finds and kills a cultist
    Given the Cultist Hunter hunts a Cultist
    Then the Cultist dies, the hunter is told "HunterFindCultist" and the cultist "CHHuntedYou"

  Scenario: The Cultist Hunter finds nothing
    Given the Cultist Hunter hunts a non-cultist
    Then the hunter is told "HunterFailedToFind"

  Scenario: The Cultist Hunter visits the Serial Killer
    Given the Cultist Hunter hunts the Serial Killer
    Then the hunter dies visiting the killer ("SerialKillerKilledCH")

  Scenario: The Cultist Hunter visits someone already dead
    Given the Cultist Hunter hunts someone killed earlier tonight
    Then the hunter is told "HunterVisitDead"

  Scenario: A hunted cultist does not convert anyone
    Given the Cultist Hunter kills the only Cultist
    Then nobody is converted tonight
