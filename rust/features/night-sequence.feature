# Scope: the night resolved as one ordered sequence, as legacy runs it
# (approved by delegation). Legacy: Werewolf.cs:2972-4468 (NightCycle),
# 2323-2430 (VisitPlayer), 2489-2509 (ConvertToCult), 1943-1950
# (BitePlayer), 4192-4433 (night death notices), 5259-5342 (menus).
#
# Out of scope here: team lists and transform notes (CursedBitten's pack
# list, cult join text, BittenTurned), which come with the role-info batch.

Feature: Night order

  Scenario: Each night resolves in the legacy order
    # covered by: engine_transcripts
    When a night resolves
    Then the regions run in this order:
      | Snow Wolf | Arsonist | Wolves | Serial Killer | Cultist Hunter | Cult |
      | Chemist   | Harlot   | Seers  | Guardian Angel | Thief         |      |
    And every visit goes through one shared visit rule
    And a player killed earlier in the night is "already dead" to later visitors

  Scenario: Nobody died
    # covered by: a_quiet_night_is_announced
    When a night ends with no deaths
    Then the group is told "NoAttack"

  Scenario: Bitten players are told after the end check
    # covered by: a_bitten_player_is_told_once_the_night_is_over
    Given the Alpha Wolf bit a player tonight and the game goes on
    Then that player is told "PlayerBitten"

Feature: The shared visit rule

  Scenario Outline: What a visit does
    # covered by: the_cult_finds_a_dead_target_or_wolves_out_hunting, a_spark_burns_the_doused_and_the_guardian_angel_saves_one, the_serial_killer_kills_a_visiting_thief_or_hunter, a_wolf_visiting_the_serial_killer_usually_dies, a_harlot_visiting_a_wolf_dies, a_guardian_angel_guarding_an_unattacked_wolf_may_die, a_stumble_into_a_grave_can_send_the_killer_elsewhere_next_night, wolves_can_fall_into_a_grave_or_spot_the_digger, a_harlot_who_went_out_is_not_home_for_the_wolves, a_guardian_angel_finds_a_harlot_out
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
    # covered by: the_arsonist_is_shown_the_houses_already_doused, engine_transcripts
    Given the Arsonist douses a player and the visit succeeds
    Then the player is doused and the Arsonist is told "Doused"

  Scenario: Sparking burns everyone doused except the GA's charge
    # covered by: a_spark_burns_the_doused_and_the_guardian_angel_saves_one, the_burned_share_one_line_when_roles_are_shown
    Given the Arsonist sparks
    Then every living doused player but the GA's charge dies (Burn), is told "Burn", and burns for the rest of the night
    And with roles shown the group gets one "Burning" line listing each victim's role

Feature: Wolves

  Scenario: The wolves' target is the most voted player, ties go to the first player listed
    # covered by: a_tied_wolf_vote_goes_to_whoever_is_listed_first, a_tied_wolf_vote_goes_to_the_first_player_in_the_list
    Given two wolves vote for different players
    Then the wolves visit the one listed first

  Scenario: One random wolf does the visiting
    # covered by: engine_transcripts
    When the wolves attack
    Then one wolf is picked at random as the visitor, and that wolf takes any visit risk

  Scenario Outline: The wolves' attack
    # covered by: the_guardian_angel_turns_the_wolves_away, the_cursed_joins_the_pack, eating_the_drunk_cancels_the_second_kill_and_the_next_night, wolves_who_ate_the_drunk_skip_the_next_night, a_hunter_shoots_back_and_dies_only_to_a_pack, the_wise_elder_survives_the_first_attack_only, the_alpha_wolf_bites_instead_of_killing, a_tied_wolf_vote_goes_to_whoever_is_listed_first, a_harlot_who_went_out_is_not_home_for_the_wolves, wolves_can_fall_into_a_grave_or_spot_the_digger, engine_transcripts
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
    # covered by: a_dead_wolf_cub_grants_the_wolves_a_bonus_kill, the_second_kill_is_asked_of_every_wolf_at_once
    Given a Wolf Cub died since the last night
    Then the wolves pick a second target, which cannot be the first

  Scenario: Wolves may spot the Grave Digger
    # covered by: wolves_can_fall_into_a_grave_or_spot_the_digger
    Given a Grave Digger dug graves last night
    Then after each wolf attack, the wolves spot and kill the Grave Digger on half the fall chance
    And the wolves are told "WolvesSpotted" and the digger "WolvesSpottedYou"

Feature: Serial Killer

  Scenario: The SK kills unless the GA guards the target
    # covered by: the_guardian_angel_stops_the_serial_killer
    Given the SK picks a player the GA guards, who is not the Harlot
    Then nobody dies and the SK is told "GuardBlockedKiller"

  Scenario: An SK who stumbled into a grave may kill someone random next night
    # covered by: a_stumble_into_a_grave_can_send_the_killer_elsewhere_next_night
    Given the SK stumbled into a grave last night and the 50% roll hits
    Then the SK kills a random living player instead, and is told "KillerRandomKill"

  Scenario: The SK may spot the Grave Digger
    # covered by: a_spotted_grave_digger_says_who_spotted_them, engine_transcripts
    Then "SerialKillerSpotted" and "SerialKillerSpottedYou", as with the wolves

Feature: Cult

  Scenario: The newest cultist visits the most voted target
    # covered by: the_newest_cultist_visits_and_dies_at_the_cultist_hunter
    Given several cultists vote
    Then the most recent convert visits, ties going to the first vote cast

  Scenario Outline: Conversion attempts
    # covered by: the_newest_cultist_visits_and_dies_at_the_cultist_hunter, the_cult_finds_a_dead_target_or_wolves_out_hunting, a_role_that_resists_is_told_about_the_attempt, engine_transcripts
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
    # covered by: the_chemist_brews_on_the_first_night_and_is_asked_on_the_second
    Given the Chemist has not brewed
    Then the Chemist is not asked tonight and is told "ChemistBrewing"
    And the next night the Chemist is asked whom to visit

  Scenario Outline: The Chemist's visit
    # covered by: the_chemist_kills_or_dies_and_brews_again, the_chemist_finds_an_empty_house, engine_transcripts
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
    # covered by: the_chemist_killing_the_wise_elder_is_announced, engine_transcripts
    Given the Chemist kills the Wise Elder and roles are shown
    Then the Chemist becomes a Villager and the group is told "ChemistKillWiseElder"

Feature: Harlot

  Scenario Outline: The Harlot's visit
    # covered by: a_harlot_who_went_out_is_not_home_for_the_wolves, a_harlot_may_discover_a_cultist, a_harlot_visiting_the_wolves_victim_dies_with_them, a_harlot_who_found_her_host_dead_names_them, a_harlot_visiting_a_wolf_dies, engine_transcripts
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
    # covered by: the_guardian_angel_turns_the_wolves_away, a_spark_burns_the_doused_and_the_guardian_angel_saves_one, the_guardian_angel_cleans_kerosene_off_a_quiet_charge, a_guardian_angel_guarding_an_unattacked_wolf_may_die, a_guardian_angel_finds_a_harlot_out, visiting_deaths_have_their_own_lines, engine_transcripts
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
    # covered by: a_spotted_grave_digger_says_who_spotted_them, visiting_deaths_have_their_own_lines, engine_transcripts
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

Feature: The Grave Digger's graves

  Scenario Outline: The Grave Digger is told whose graves they dig
    # covered by: the_grave_digger_is_told_whose_graves_to_dig, graves_are_listed_with_and
    Given <deaths> since the last night the Grave Digger dug
    When the night's questions go out
    Then the Grave Digger is told <told>
    Examples:
      | deaths                                  | told                                         |
      | nobody died (fleeing doesn't count)     | "DigNoGraves"                                |
      | two players died                        | "DigGraves" naming them, joined with "And"   |

  Scenario: No graves on a night the Sandman slept
    # covered by: engine_transcripts
    Given the Sandman put the village to sleep
    Then the Grave Digger is told nothing, and those deaths still count next night

Feature: Night menus offer what the legacy menus offered
  # Werewolf.cs:5148-5300 (SendNightActions), 5094 (the Blacksmith),
  # 1030-1050 and 1112-1122 (teammates told how each one voted).

  Scenario: The Serial Killer is asked every night
    # covered by: the_serial_killer_kills_at_night, night_menus_leave_out_the_player_and_their_team
    Given a living Serial Killer
    When night falls
    Then they are asked "AskKill" with every other living player

  Scenario Outline: Nobody is offered themselves or their own team
    # covered by: night_menus_leave_out_the_player_and_their_team, the_arsonist_is_shown_the_houses_already_doused
    Given a living <role>
    When night falls
    Then their menu offers <offered>
    Examples:
      | role         | offered                                                        |
      | Seer         | every other living player                                      |
      | wolf         | every living player who is not a wolf or the Snow Wolf         |
      | Cultist      | every living player who is not a Cultist                       |
      | Arsonist     | every other living player not yet doused                       |
      | Cupid        | every living player, themselves included                       |

  Scenario: The Blacksmith's question has its own text
    # covered by: night_menus_leave_out_the_player_and_their_team
    Given a Blacksmith who hasn't spread silver
    Then they are asked "SpreadDust" during the day

  Scenario Outline: Teammates hear each vote as it's cast
    # covered by: teammates_hear_each_vote_as_it_is_cast
    Given two living <team>
    When one of them picks a target
    Then the other is told "<key>" naming the voter and the target, before the night ends
    Examples:
      | team      | key                  |
      | wolves    | WolfVotedKill        |
      | Cultists  | CultistVotedConvert  |

  Scenario: The wolves' second kill is asked of every wolf at once
    # covered by: the_second_kill_is_asked_of_every_wolf_at_once
    Given the Wolf Cub died the day before
    When night falls
    Then every eating wolf gets the second menu in one round of questions
    And the other wolves hear each second vote with "WolfVotedKill"
