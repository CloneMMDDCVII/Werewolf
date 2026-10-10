# Scope: the night resolved as one ordered sequence, as legacy runs it
# (approved by delegation). Legacy: Werewolf.cs:2972-4468 (NightCycle),
# 2323-2430 (VisitPlayer), 2489-2509 (ConvertToCult), 1943-1950
# (BitePlayer), 4192-4433 (night death notices), 5259-5342 (menus).
#
# Out of scope here: team lists and transform notes (CursedBitten's pack
# list, cult join text, BittenTurned), which come with the role-info batch.

Feature: Night order

  Scenario: Each night resolves in the legacy order
    When a night resolves
    Then the regions run in this order:
      | Snow Wolf | Arsonist | Wolves | Serial Killer | Cultist Hunter | Cult |
      | Chemist   | Harlot   | Seers  | Guardian Angel | Thief         |      |
    And every visit goes through one shared visit rule
    And a player killed earlier in the night is "already dead" to later visitors

  Scenario: Nobody died
    When a night ends with no deaths
    Then the group is told "NoAttack"

  Scenario: Bitten players are told after the end check
    Given the Alpha Wolf bit a player tonight and the game goes on
    Then that player is told "PlayerBitten"

Feature: The shared visit rule

  Scenario Outline: What a visit does
    Given <visitor> visits <target>
    Then <outcome>
    Examples:
      | visitor            | target                                   | outcome                                                     |
      | anyone             | someone who died earlier tonight         | the visit finds them already dead                           |
      | the Serial Killer  | anyone but a Grave Digger                | the visit succeeds                                          |
      | anyone but the SK  | a player burning tonight                 | the visitor dies (VisitBurning)                             |
      | a non-wolf         | the Serial Killer                        | the visitor dies (VisitKiller)                              |
      | a wolf             | the Serial Killer who went out           | the wolf survives on a 20% roll, else dies (VisitKiller)    |
      | the Harlot         | a wolf or the Snow Wolf                  | the Harlot dies (VisitWolf)                                 |
      | the GA             | a wolf nobody attacked                   | the GA dies on a 50% roll (GuardWolf)                       |
      | the Serial Killer  | a Grave Digger who dug                   | the SK stumbles, is told "KillerStumbled", the visit succeeds |
      | anyone else        | a Grave Digger who dug                   | the visitor may fall (FallGrave), otherwise nobody is home  |
      | anyone             | a Harlot, or GA (not for wolves), who went out | nobody is home                                        |

Feature: Arsonist

  Scenario: Dousing goes through the visit rule
    Given the Arsonist douses a player and the visit succeeds
    Then the player is doused and the Arsonist is told "Doused"

  Scenario: Sparking burns everyone doused except the GA's charge
    Given the Arsonist sparks
    Then every living doused player but the GA's charge dies (Burn), is told "Burn", and burns for the rest of the night
    And with roles shown the group gets one "Burning" line listing each victim's role

Feature: Wolves

  Scenario: The wolves' target is the most voted player, ties go to the first player listed
    Given two wolves vote for different players
    Then the wolves visit the one listed first

  Scenario: One random wolf does the visiting
    When the wolves attack
    Then one wolf is picked at random as the visitor, and that wolf takes any visit risk

  Scenario Outline: The wolves' attack
    Given the wolves attack <target>
    Then <outcome>
    Examples:
      | target                               | outcome                                                                 |
      | the GA's charge                      | every wolf is told "GuardBlockedWolf"; nothing else is checked          |
      | the Cursed                           | the Cursed turns Wolf                                                   |
      | the Drunk                            | the Drunk dies; wolves are told "WolvesEatDrunk" and skip the next night and any second kill |
      | the Hunter, who wins the 30%+20%/extra wolf roll | a random wolf is shot; the Hunter dies too only if several wolves voted |
      | the Wise Elder, first time           | nobody dies; "WolvesTriedToEatWiseElder" and "WolvesAteWiseElderPM"     |
      | anyone, with an Alpha Wolf's 20% bite | the target is bitten instead, and the pack is told "PlayerBittenWolves" |
      | anyone else                          | the target dies (Eat) and is told "WolvesEatYou"                        |
      | a Harlot who went out                | wolves are told "HarlotNotHome"                                         |
      | a Grave Digger, and nobody falls     | wolves are told "GraveDiggerNotHome"                                    |
      | a Grave Digger, and the visitor falls| wolves are told "WolfFellWolves"                                        |
      | a burning player                     | the visitor dies; wolves are told "WolvesVisitBurn"                     |

  Scenario: A Wolf Cub's death gives a second kill the same night
    Given a Wolf Cub died since the last night
    Then the wolves pick a second target, which cannot be the first

  Scenario: Wolves may spot the Grave Digger
    Given a Grave Digger dug graves last night
    Then after each wolf attack, the wolves spot and kill the Grave Digger on half the fall chance
    And the wolves are told "WolvesSpotted" and the digger "WolvesSpottedYou"

Feature: Serial Killer

  Scenario: The SK kills unless the GA guards the target
    Given the SK picks a player the GA guards, who is not the Harlot
    Then nobody dies and the SK is told "GuardBlockedKiller"

  Scenario: An SK who stumbled into a grave may kill someone random next night
    Given the SK stumbled into a grave last night and the 50% roll hits
    Then the SK kills a random living player instead, and is told "KillerRandomKill"

  Scenario: The SK may spot the Grave Digger
    Then "SerialKillerSpotted" and "SerialKillerSpottedYou", as with the wolves

Feature: Cult

  Scenario: The newest cultist visits the most voted target
    Given several cultists vote
    Then the most recent convert visits, ties going to the first vote cast

  Scenario Outline: Conversion attempts
    Given the cult visits <target>
    Then <outcome>
    Examples:
      | target                         | outcome                                                                   |
      | a Hunter                       | 50% converts; else 50% the Hunter kills the visitor ("CultConvertHunter") |
      | the Cultist Hunter             | the visitor dies ("CultConvertCultHunter", "CultHunterKilledCultVisit")   |
      | a wolf whose pack stayed home  | the visitor dies ("CultConvertWolf", "CultAttempt")                       |
      | a wolf whose pack went out     | "CultVisitEmpty"                                                          |
      | a Grave Digger                 | converts on a 30% roll, if nobody falls                                   |
      | an Arsonist who went out       | "CultVisitEmpty"                                                          |
      | a role that resists            | "CultUnableToConvert" to the cult and "CultAttempt" to the target         |
      | someone already dead           | "CultTargetDead"                                                          |
      | the Serial Killer              | the visitor dies ("CultConvertSerialKiller")                              |

Feature: Chemist

  Scenario: The Chemist brews one night and visits the next
    Given the Chemist has not brewed
    Then the Chemist is not asked tonight and is told "ChemistBrewing"
    And the next night the Chemist is asked whom to visit

  Scenario Outline: The Chemist's visit
    Given the Chemist visits <target>
    Then <outcome>
    Examples:
      | target                    | outcome                                                                 |
      | a living player, 50% hit  | the target dies; "ChemistSuccess" and "ChemistVisitYouSuccess"         |
      | a living player, 50% miss | the Chemist dies; "ChemistFail" and "ChemistVisitYouFail"              |
      | someone already dead      | "ChemistTargetDead"                                                     |
      | someone not home          | "ChemistTargetEmpty"                                                    |
      | the Serial Killer         | the Chemist dies; "ChemistSK" and "ChemistVisitYouSK"                  |

  Scenario: Killing the Wise Elder costs the Chemist their role
    Given the Chemist kills the Wise Elder and roles are shown
    Then the Chemist becomes a Villager and the group is told "ChemistKillWiseElder"

Feature: Harlot

  Scenario Outline: The Harlot's visit
    Given the Harlot visits <target>
    Then <outcome>
    Examples:
      | target                                   | outcome                                                       |
      | a living non-cultist                     | "HarlotVisitNonWolf" to her, "HarlotVisitYou" to the target    |
      | a cultist, 50% discovery                 | "HarlotDiscoverCult"                                           |
      | someone the wolves or SK killed tonight  | she dies too (VisitVictim), named with her host in the group line |
      | someone who died another way tonight     | "HarlotVisitNotHome"                                           |
      | a wolf                                   | she dies; "HarlotFuckWolf"                                     |
      | a Grave Digger, no fall                  | "HarlotVisitDigger"                                            |

Feature: Guardian Angel

  Scenario Outline: What the GA is told
    Given the GA guarded <target>
    Then <outcome>
    Examples:
      | target                                    | outcome                                         |
      | someone who was attacked                  | "GuardSaved" and "GuardSavedYou"                |
      | someone saved from the Arsonist's fire    | "GuardSavedFromFire" and "GuardSavedYouFromFire" |
      | a doused player nobody attacked           | the kerosene is cleaned: "CleanDoused"          |
      | someone nobody attacked                   | "GuardNoAttack"                                 |
      | a Harlot who went out                     | "GuardEmptyHouse"                               |
      | a wolf, and the GA died                   | "GuardWolf"; the group is told "GAGuardedWolf"  |
      | the Serial Killer                         | the GA dies; "GuardKiller", "GAGuardedKiller"   |

Feature: Night death lines

  Scenario Outline: Group lines for deaths by visiting
    Given roles are shown on death and <victim> died by <cause>
    Then the group is told "<key>"
    Examples:
      | victim           | cause                         | key                        |
      | the Grave Digger | being spotted by the SK       | KillerSpottedDiggerPublic  |
      | the Grave Digger | being spotted by the wolves   | WolvesSpottedDiggerPublic  |
      | a wolf           | visiting a burning player     | WolfVisitBurn              |
      | anyone else      | visiting a burning player     | DefaultVisitBurn           |
      | a Cultist        | visiting a wolf               | CultConvertWolfPublic      |
      | the Chemist      | visiting the SK               | ChemistSKPublic            |
      | the Arsonist     | visiting the SK               | ArsonistVisitKiller        |
