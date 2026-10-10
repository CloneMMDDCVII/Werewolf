# Scope: day abilities take effect the moment they're used (approved by
# delegation). Legacy: Werewolf.cs:899-973 (the day callbacks, each of
# which announces on the spot), 2548-2610 (the lynch loop and the
# Pacifist's peace closing the lynch menus),
# 2860-2898 (the Gunner's shot, which kills at once).

Feature: Day abilities are announced when used

  Scenario Outline: Announced as soon as the player answers
    # covered by: each_ability_is_announced_as_soon_as_it_is_used
    Given the day's questions are out
    When the <role> answers yes
    Then the group is told "<key>" before anyone else's answer lands
    Examples:
      | role         | key                     |
      | Mayor        | MayorReveal             |
      | Pacifist     | PacifistNoLynch         |
      | Blacksmith   | BlacksmithSpreadSilver  |
      | Troublemaker | TroublemakerDoubleLynch |
      | Sandman      | SandmanSleepAll         |

  Scenario: Nothing is announced again once the day resolves
    # covered by: nothing_is_announced_again_after_the_day
    Given the Mayor revealed during the day
    When the day resolves
    Then "MayorReveal" was told once

  Scenario: The Mayor and the Pacifist can act any time until the vote ends
    # covered by: a_day_asks_every_vote_and_day_action_in_one_batch, a_revealed_mayor_s_doubled_vote_breaks_a_tie
    Given the Mayor has not revealed by the end of the discussion
    Then the Mayor's question stays open through the lynch vote
    And a Mayor who reveals mid-vote counts twice in that vote

Feature: The Pacifist's peace stops the vote

  Scenario: Peace during the discussion means no vote at all
    # covered by: peace_closes_the_votes_still_open, a_game_with_peace_lynches_nobody_and_says_so
    Given the Pacifist makes peace before the lynch vote opens
    Then no lynch vote is sent
    And the group is told "PacifistNoLynchNow" where the vote would have started

  Scenario: Peace during the vote closes it
    # covered by: peace_closes_the_votes_still_open, a_game_with_peace_lynches_nobody_and_says_so, the_pacifists_peace_closes_the_lynch_vote
    Given the lynch vote is open and some players have voted
    When the Pacifist makes peace
    Then every open lynch question is closed with "LynchPeaceTimeout"
    And the group is told "PacifistNoLynchNow"
    And nobody is lynched, and no vote results are shown

  Scenario: A Troublemaker after the peace brings the vote back
    # covered by: trouble_after_peace_brings_the_vote_back, the_troublemaker_causes_a_second_lynch_the_same_day
    Given the Pacifist made peace
    When the Troublemaker then makes trouble
    Then the lynch vote goes ahead, twice

Feature: The Gunner's shot kills at once

  Scenario: The shot is announced and the victim is out of the day
    # covered by: the_gunners_shot_kills_at_once, a_shot_is_told_once_in_a_whole_game
    Given the day's questions are out
    When the Gunner shoots a player
    Then the shot's death line is told at once (Werewolf.cs:2873-2898)
    And the victim's open questions close, and they can't vote
    And nobody can vote for them, and votes already cast by or for them are dropped

  Scenario: Shooting the Wise Elder
    # covered by: shooting_the_wise_elder_is_told_right_after_the_shot
    When the Gunner shoots the Wise Elder
    Then "GunnerShotWiseElder" follows the shot's line at once

  Scenario: What still waits for the end of the day
    # covered by: engine_transcripts
    Given the Gunner shot someone
    Then their lover's grief, a shot Hunter's last shot and any role changes come once the day's questions close
