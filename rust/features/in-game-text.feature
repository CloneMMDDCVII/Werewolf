# Scope: the remaining in-game text (approved by delegation, 10 Oct 2026).
# Achievement text is out with achievements. Lobby text (joining, extend,
# flee replies, idle kills, Yes/No/Skip buttons, "Choice accepted") belongs
# to the bot's presenter, not the engine.
#
# Shape: the engine picks every legacy line and hands presenters a
# `messages::Message` (key plus arguments), so presenters only render.
# Deaths carry their line in `NarrationEvent::Death::message`, chosen by
# `death_text::death_message`. Private lines are `Note`s, group lines are
# `Announcement`s.

Feature: Day abilities announce themselves

  Scenario Outline: A day ability is announced when it is used
    When the <role> uses their ability
    Then the group is told "<key>" naming them
    Examples:
      | role         | key                      |
      | Mayor        | MayorReveal              |
      | Pacifist     | PacifistNoLynch          |
      | Blacksmith   | BlacksmithSpreadSilver   |
      | Troublemaker | TroublemakerDoubleLynch  |

  Scenario: The Pacifist's peace replaces the lynch result
    Given the Pacifist called for peace today
    When the lynch would be resolved
    Then the group is told "PacifistNoLynchNow" and nobody is lynched

  Scenario: The Prince survives their first lynch
    When the village lynches the Prince for the first time
    Then the group is told "PrinceLynched" and the Prince lives

Feature: Every death gets its legacy line

  Scenario Outline: The line depends on how the player died
    When <victim> dies by <method>
    Then the group is told "<key>" with <args>
    Examples:
      | victim               | method                  | key                       | args                          |
      | anyone               | lynch                   | LynchKill                 | victim, reveal                |
      | anyone               | the Gunner's shot       | DefaultShot               | gunner, victim, reveal        |
      | the Harlot (shown)   | the Gunner's shot       | HarlotShot                | gunner, victim                |
      | anyone               | a lynched Hunter's shot | HunterKilledFinalLynched  | hunter, victim, reveal        |
      | anyone               | a killed Hunter's shot  | HunterKilledFinalShot     | hunter, victim, reveal        |
      | a wolf               | the Hunter at night     | HunterShotWolf(Multi)     | victim, then "PlayerRoleWas"  |
      | the Snow Wolf        | the Hunter at night     | SnowFrozeHunter           | victim                        |
      | the cult's recruit   | the Hunter              | HunterKilledVisiter       | victim, "role is dead"        |
      | a lover              | grief                   | LoverDied                 | first lover, victim, reveal   |
      | anyone               | fleeing                 | Flee                      | victim, then "PlayerRoleWas" when shown |

  Scenario: Night deaths say nothing about roles when roles are hidden
    Given show roles on death is off
    When a player dies at night
    Then the group is told "GenericDeathNoReveal"

  Scenario: A Hunter who shoots nobody says so
    When a dying Hunter skips their shot
    Then the group is told "HunterSkipChoiceLynched" or "HunterSkipChoiceShot"

  Scenario: A Hunter who shoots the Wise Elder
    When a dying Hunter shoots the Wise Elder
    Then the group is told "HunterKilledWiseElder" after the shot

Feature: Night results, transforms, team lists and endings
  # Built in batches from the research tables; each batch adds its own
  # scenarios here as it lands.
