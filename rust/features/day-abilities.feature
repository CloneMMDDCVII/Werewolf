# Scope: day abilities take effect the moment they're used (approved by
# delegation). Legacy: Werewolf.cs:899-973 (the day callbacks, each of
# which announces on the spot), 2548-2610 (the lynch loop and the
# Pacifist's peace closing the lynch menus).
#
# Out of scope here: the Gunner's shot killing at once (the victim
# still votes until the day resolves), which needs deaths mid-batch.

Feature: Day abilities are announced when used

  Scenario Outline: Announced as soon as the player answers
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
    Given the Mayor revealed during the day
    When the day resolves
    Then "MayorReveal" was told once

  Scenario: The Mayor and the Pacifist can act any time until the vote ends
    Given the Mayor has not revealed by the end of the discussion
    Then the Mayor's question stays open through the lynch vote
    And a Mayor who reveals mid-vote counts twice in that vote

Feature: The Pacifist's peace stops the vote

  Scenario: Peace during the discussion means no vote at all
    Given the Pacifist makes peace before the lynch vote opens
    Then no lynch vote is sent
    And the group is told "PacifistNoLynchNow" where the vote would have started

  Scenario: Peace during the vote closes it
    Given the lynch vote is open and some players have voted
    When the Pacifist makes peace
    Then every open lynch question is closed with "LynchPeaceTimeout"
    And the group is told "PacifistNoLynchNow"
    And nobody is lynched, and no vote results are shown

  Scenario: A Troublemaker after the peace brings the vote back
    Given the Pacifist made peace
    When the Troublemaker then makes trouble
    Then the lynch vote goes ahead, twice
