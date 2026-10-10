# Scope: Thief full mode and Cultist Hunter results (approved by delegation).
# Legacy: Werewolf.cs:1977, 2212-2264 (StealRole), 2349, 2353-2376,
# 4132-4190 (Thief Night), 3556-3590 (Cult Hunter Night), 5290-5330 (menus).

Feature: Thief full mode

  Background:
    Given the group plays with ThiefFull on

  Scenario: The Thief may try to steal every night, and may skip
    # covered by: a_full_mode_thief_tries_every_night_and_never_robs_a_wolf
    When night falls
    Then the Thief is asked "AskThief" with a Skip option, every night

  Scenario: A successful steal swaps roles and the victim becomes the Thief
    # covered by: a_landed_steal_makes_the_victim_the_new_thief, a_full_steal_lands_on_a_good_roll
    Given the Thief visits a Seer and the 50% roll succeeds
    Then the Thief becomes the Seer and the Seer becomes the Thief
    # Werewolf.cs:1977 only turns the victim into a Villager outside full mode.

  Scenario Outline: Steals that fail
    # covered by: a_full_steal_lands_on_a_good_roll, wolves_and_cultists_cannot_be_stolen_from, the_serial_killer_kills_a_visiting_thief_or_hunter, a_grave_digger_can_trap_the_thief, engine_transcripts
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
    # covered by: a_frozen_full_mode_thief_steals_nothing, only_a_full_mode_thief_is_stopped_by_the_freeze
    Given the Snow Wolf froze the Thief
    Then no steal happens and the Thief was told "ThiefFrozen"

Feature: Cultist Hunter results

  Scenario: The Cultist Hunter finds and kills a cultist
    # covered by: the_cultist_hunter_kills_a_cultist_before_the_cult_converts, the_cultist_hunter_finds_a_cultist_or_nothing
    Given the Cultist Hunter hunts a Cultist
    Then the Cultist dies, the hunter is told "HunterFindCultist" and the cultist "CHHuntedYou"

  Scenario: The Cultist Hunter finds nothing
    # covered by: the_cultist_hunter_learns_a_target_is_not_a_cultist, the_cultist_hunter_finds_a_cultist_or_nothing
    Given the Cultist Hunter hunts a non-cultist
    Then the hunter is told "HunterFailedToFind"

  Scenario: The Cultist Hunter visits the Serial Killer
    # covered by: the_serial_killer_kills_a_visiting_thief_or_hunter
    Given the Cultist Hunter hunts the Serial Killer
    Then the hunter dies visiting the killer ("SerialKillerKilledCH")

  Scenario: The Cultist Hunter visits someone already dead
    # covered by: engine_transcripts
    Given the Cultist Hunter hunts someone killed earlier tonight
    Then the hunter is told "HunterVisitDead"

  Scenario: A hunted cultist does not convert anyone
    # covered by: the_cultist_hunter_kills_a_cultist_before_the_cult_converts
    Given the Cultist Hunter kills the only Cultist
    Then nobody is converted tonight
