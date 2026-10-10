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

Feature: The game ends the way the legacy game ends
  # game_end::check_game_end ports CheckForGameEnd and DoGameEnd for a live
  # game; lib::evaluate_winner stays the RNG-free check fixture replays use.

  Scenario Outline: Last-two standoffs
    Given only <a> and <b> are alive, not in love
    Then <ending>
    Examples:
      | a        | b               | ending                                                                    |
      | a Hunter | a wolf          | 30%: "HunterKillsWolfEnd", the wolf dies, village wins; else "WolfKillsHunterEnd", wolves win |
      | a Hunter | the SK          | "SKHunterEnd", both die, "NoWinner"                                        |
      | a Hunter | a Hunter        | the village wins                                                          |
      | anyone   | the SK          | "SerialKillerWinsOverpower", the other dies, the SK wins                  |
      | anyone   | the Arsonist    | "ArsonistWinsOverpower" unless a Gunner with bullets is one of them       |
      | a Cultist| a Cultist       | the cult wins                                                             |
      | a Cultist| a wolf          | the wolves win                                                            |
      | a Cultist| the CH          | "CHKillsCultistEnd", the Cultist dies, the village wins                   |
      | a Cultist| anyone else     | they are converted (not a Thief or Doppelgänger) and the cult wins        |

  Scenario: A Gunner with bullets holds off level wolves
    Given the wolves equal the others and a Gunner has a bullet left
    Then the game goes on

  Scenario: Neutral endings name who was left
    Given only neutral roles are left
    Then each gets "SorcererEnd", "ThiefEnd", "DoppelgangerEnd" or "TannerEnd"
    And a Tanner left with another neutral role dies first
    And the winner line is "NoWinner"

  Scenario: Several wolves win together
    When the wolves win with more than one wolf left
    Then the winner line is "WolvesWin" instead of "WolfWins"

Feature: Night results, transforms, team lists and endings
  # Built in batches from the research tables; each batch adds its own
  # scenarios here as it lands.
