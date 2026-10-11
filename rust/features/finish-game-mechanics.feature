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
    # covered by: the_menu_skips_wolves_and_last_nights_target, the_snow_wolfs_freeze_goes_out_with_the_night
    When night begins and Blacksmith silver was not spread today
    Then the Snow Wolf is asked "AskFreeze"
    And the options exclude every wolf-team role and the player frozen last night

  Scenario: Silver spread means no freeze tonight
    # covered by: silver_means_no_freeze_question
    Given the Blacksmith spread silver today
    Then the Snow Wolf is not asked to freeze

  Scenario Outline: A frozen player's action fails that same night
    # covered by: a_frozen_seer_loses_their_reading, a_frozen_harlot_stays_home, a_frozen_guardian_angel_protects_nobody, a_frozen_seer_learns_nothing_and_cannot_be_frozen_twice_in_a_row, engine_transcripts
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
    # covered by: engine_transcripts
    Given two Cultists and one is frozen
    Then only the unfrozen Cultist's choice counts (Werewolf.cs:3594)

  Scenario: A frozen Grave Digger's graves are not dug
    # covered by: the_grave_digger_notice_depends_on_dug_graves, engine_transcripts
    Given the Grave Digger dug graves and is frozen
    Then no grave-fall deaths happen tonight and the digger is told "GraveDiggerFrozen"

  Scenario: The Serial Killer is frozen and does not kill
    # covered by: the_serial_killer_is_frozen_when_the_snow_wolf_survives_the_visit
    Given the Snow Wolf freezes the Serial Killer
    Then the Serial Killer is told "SKFrozen" and makes no kill tonight

  Scenario: The Arsonist ignores the freeze
    # covered by: the_arsonist_acts_despite_the_freeze
    Given the Snow Wolf freezes the Arsonist
    Then the Arsonist acts normally and is told "ArsonistNotFrozen"
    And the Arsonist cannot be frozen again the next night

  Scenario: The Guardian Angel blocks the freeze
    # covered by: a_guardian_angel_blocks_the_freeze
    Given the Guardian Angel protects the Snow Wolf's target
    Then the target is not frozen and the Snow Wolf is told "GuardBlockedSnowWolf"

  Scenario Outline: Freezing a Hunter is a coin flip (RngSource)
    # covered by: a_hunter_is_frozen_or_shoots_the_snow_wolf
    Given the Snow Wolf freezes the Hunter and the roll is <roll>
    Then <outcome>
    Examples:
      | roll  | outcome                                                   |
      | < 50  | the Hunter is frozen and told "DefaultFrozen"             |
      | >= 50 | the Snow Wolf dies by HunterShot and the Hunter is free   |

  Scenario: Freeze wears off
    # covered by: the_menu_skips_wolves_and_last_nights_target, a_frozen_seer_learns_nothing_and_cannot_be_frozen_twice_in_a_row
    Given a player was frozen last night
    Then they act normally tonight but cannot be the Snow Wolf's target tonight

  Scenario: The last wolf standing turns the Snow Wolf into a Wolf (Werewolf.cs:4487-4496)
    # covered by: the_last_snow_wolf_becomes_a_wolf_and_starts_eating
    Given every other wolf-team player is dead
    Then the Snow Wolf transforms into a Wolf and is told "SnowWolfTurnWolf"

Feature: Seer-family result messages (Werewolf.cs:3922-4062)

  Scenario Outline: The Seer sees a role, with legacy disguises
    # covered by: the_seer_sees_true_roles_and_legacy_disguises, the_traitor_reads_as_wolf_or_villager
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
    # covered by: the_sorcerer_only_recognises_wolves_and_seers
    Given the Sorcerer checks a <target>
    Then the Sorcerer is told "<key>"
    Examples:
      | target   | key                    |
      | WolfCub  | SeerSees (Wolf)        |
      | Seer     | SeerSees (Seer)        |
      | SnowWolf | SeerSees (SnowWolf)    |
      | Lycan    | SorcererOther          |

  Scenario: The Fool sees a random living role
    # covered by: the_fool_sees_a_random_non_seer_role_with_wolf_types_flattened
    Given the Fool checks anyone
    Then the Fool is told "SeerSees" with a random living non-Seer role (wolf types shown as Wolf)

  Scenario: The Oracle sees a role the target is not
    # covered by: the_oracle_sees_a_role_the_target_is_not
    Given the Oracle checks a target
    Then the Oracle is told "NegSeerSees" with a random living role other than the target's
    And if no such role exists the Oracle is told nothing

  Scenario: The Augur sees a role that is not in play
    # covered by: the_augur_sees_each_absent_role_once_then_nothing, the_augur_is_shown_the_apprentice_instead_of_a_dead_seer
    When night resolves
    Then the Augur is told "AugurSees" with a role not held by anyone alive and not seen before
    And when none remain the Augur is told "AugurSeesNothing"
    And Seer is replaced by Apprentice Seer when the Seer is dead and an Apprentice is alive

  Scenario: The Detective snoops by day, not by night (Werewolf.cs:2933-2953)
    # covered by: the_detective_snoops_by_day, the_detective_learns_the_exact_role_and_may_be_caught
    Given the Detective picks a target during the day
    Then after the day the Detective is told "DetectiveSnoop" with the exact role
    And with 40% chance (RngSource) every wolf is told "DetectiveCaught"
    # Correction: the current port asks the Detective at night.

Feature: Troublemaker's double lynch (Werewolf.cs:913-973, 2540-2846)

  Scenario: Trouble causes a second lynch vote the same day
    # covered by: the_troublemaker_causes_a_second_lynch_the_same_day
    Given the Troublemaker chooses trouble
    Then the group is told "TroublemakerDoubleLynch"
    And after the first lynch resolves, the group is told "TroubleDoubleLynchNow"
    And a second full lynch vote is held among the survivors

  Scenario: The game can end between the two lynches
    # covered by: engine_transcripts
    Given the first lynch ends the game
    Then no second vote is held

  Scenario: Whoever acts last wins between Pacifist and Troublemaker
    # covered by: a_pacifist_acting_after_the_troublemaker_cancels_both_lynches, pacify_after_trouble_cancels_the_lynch, peace_after_trouble_wins_even_when_the_pacifist_is_listed_first
    Given the Pacifist calls peace after the Troublemaker called trouble
    Then there is no lynch today
    # Current port always lets trouble win; C# is order-dependent
    # (917 "peace overrides trouble", 971 "trouble overrides peace").

Feature: Lynch outcomes (Werewolf.cs:2690-2797)
  # Note: legacy has no re-vote on a tie. RandomLynch is hard-coded false, so a
  # tie means nobody dies. The only "re-vote" is Troublemaker's second lynch.

  Scenario: A tied vote kills nobody
    # covered by: a_tied_lynch_kills_nobody_and_says_so, tied_lynch_vote_kills_nobody
    Given two players tie for most votes
    Then nobody is lynched and the group is told "LynchTie"

  Scenario: No votes at all
    # covered by: nobody_voting_is_announced
    Given nobody votes
    Then nobody is lynched and the group is told "NoLynchVotes"

  Scenario: Clumsy Guy's vote may go astray (Werewolf.cs:999-1011)
    # covered by: a_clumsy_guys_vote_can_go_astray
    Given the Clumsy Guy votes for a player
    Then with 50% chance (RngSource) the vote goes to a random living player other than themselves

Feature: Hunter domino and Gunner bullets

  Scenario: A Hunter shot by a Hunter gets their own final shot (Werewolf.cs:5480-5481)
    # covered by: a_hunter_shot_by_a_hunter_shoots_too
    Given Hunter A's final shot kills Hunter B
    Then Hunter B is asked for a final shot too, and so on down the chain

  Scenario: A Gunner's bullets are a count, not two flags (Werewolf.cs:1938, 2009, 2479-2486)
    # not tested: gap
    Given a Thief steals a Gunner who has fired once
    Then the Thief becomes a Gunner with one bullet left
    And the Gunner is asked "AskShoot" with the remaining bullet count

# Out of scope here (listed for the parity tracker):
#   Thief full mode (needs group config / chaos-mode flag), Witch (disabled),
#   secret lynch vote display, achievements, Telegram presenter, database.
