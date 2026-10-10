# Scope: make the engine read the group settings group config stores
# (approved by delegation, 10 Oct 2026). Legacy: Werewolf.cs:150-216 (where
# the node reads each flag), 1125-1137, 2560-2800, 2825-2831, 3003-3028,
# 4906-4925.
#
# Shape: a Telegram-free `game_engine::rules::GameRules` handed to
# `run_game_with_rules`; `control::game_rules::game_start(settings, requested_mode, rng)`
# builds it from `GroupSettings`, resolving Random mode. run_game keeps the
# legacy defaults so fixtures and existing tests are unchanged.
#
# Display-only settings stay with the presenter, which reads them from the
# group: ShowIDs (names in /players and "HaveJoined"), ShufflePlayerList
# (button and list order) and AllowNSFW (custom start GIFs). The engine has
# nothing to decide for them.
#
# Divergence, stated: in Random mode the legacy node rolls ShowRolesOnDeath
# and ShowRolesEnd but then reads the stored flags anyway (Werewolf.cs:855,
# 2777, 4906). We use the rolled values, which is what Random mode is for.

Feature: The engine follows the group's game settings

  Scenario Outline: Phase timers are announced with the group's durations
    Given the group's <phase> timer is <stored> seconds
    When the <phase> starts with <alive> players alive
    Then the group is told "<key>" with <shown> seconds
    Examples:
      | phase | stored | alive | key       | shown | note                                          |
      | night | 90     | 8     | NightTime | 90    |                                               |
      | night | 60     | 8     | NightTime | 120   | night 1 with Cupid, WildChild, DG or a Thief  |
      | day   | 60     | 8     | DayTime   | 120   | plus max((alive/5 - 1) * 30, 60)               |
      | day   | 60     | 20    | DayTime   | 150   |                                               |
      | lynch | 90     | 8     | LynchTime | 90    |                                               |

  Scenario: The day number is announced with the day timer
    When day 2 starts
    Then the group is told "Day" with 2

  Scenario: Public lynch votes are announced as they are cast
    Given secret lynch is off
    When A votes to lynch B
    Then the group is told "PlayerVotedLynch" with A and B

  Scenario: Secret lynch only shows how many have voted
    Given secret lynch is on
    When 3 of 7 living players have voted
    Then the group is told "PlayerVoteCounts" with 3 and 7
    And nobody is told who voted for whom

  Scenario Outline: Secret lynch results after the vote
    Given secret lynch is on, show votes is <votes> and show voters is <voters>
    When the vote ends with B on 3 votes from A, C and a revealed Mayor
    Then the group is told <result>
    Examples:
      | votes | voters | result                                                                 |
      | off   | any    | nothing extra                                                          |
      | on    | off    | "SecretLynchResultFull" listing "SecretLynchResultNumber" lines        |
      | on    | on     | "SecretLynchResultFull" listing "SecretLynchResultEach" with "Mayor (2)" |

  Scenario Outline: Roles are revealed on death only when the group allows it
    Given show roles on death is <setting>
    When a Seer is lynched
    Then the death message <reveals> the role
    Examples:
      | setting | reveals      |
      | on      | reveals      |
      | off     | does not say |

  Scenario Outline: The end-of-game summary follows ShowRolesEnd
    Given ShowRolesEnd is <setting>
    When the game ends
    Then the summary lists <who>
    Examples:
      | setting | who                                                                 |
      | None    | "PlayersAlive" count and every name, no roles                       |
      | Living  | "RemainingPlayersEnd": survivors with role, team and Won/Lost       |
      | All     | "PlayersAlive" count and everyone: Alive/Dead/RanAway, role, Won/Lost |

  Scenario: Random mode rolls the per-game settings
    Given the group has Random mode on
    When a game starts
    Then thief full, secret lynch, show votes, show voters and show roles on
      death are each a 50/50 roll, and ShowRolesEnd is None, Living or All
      with roughly equal odds

  Scenario: Thief full mode is read from the group (behaviour in thief-full.feature)
    Given the group has ThiefFull on
    Then the engine runs the Thief in full mode
