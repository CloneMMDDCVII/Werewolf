# Scope: what players are told privately about their role, at the start
# and whenever it changes (approved by delegation). Legacy: Werewolf.cs
# 1612-1711 (NotifyRoles, GetRoleInfo), 1753-1779 (role models picked for
# those who didn't), 1963-2260 (Transform's messages), 5220-5237 (role
# model menus).
#
# The engine builds these messages; the bot only sends them.
#
# Not here: the legacy quirks of listing a bitten player in their own
# pack list and telling cultists "CultistBitten" twice; a Doppelganger who
# copies a Wild Child inheriting its role model (NewWCRoleModel).

Feature: Role info when roles are dealt

  Scenario Outline: Every player is told their role
    # covered by: role_info_matches_get_role_info, teams_are_listed_when_there_is_more_than_one, everyone_is_told_their_role_before_anything_else
    Given a player is dealt <role>
    Then they are told "<key>"<extra>
    Examples:
      | role          | key                | extra                                                                 |
      | Villager      | RoleInfoVillager   |                                                                       |
      | Fool          | RoleInfoSeer       | , so they believe they're the Seer                                    |
      | Thief         | RoleInfoThief      | , or "RoleInfoThiefFull" when the Thief steals every night            |
      | Beholder      | RoleInfoBeholder   | , then "BeholderSeer" naming the Seer, or "NoSeer"                    |
      | Mason         | RoleInfoMason      | , then "MasonTeam" listing the masons when there's more than one      |
      | Cultist       | RoleInfoCultist    | , then "CultistsList" when there's more than one                      |
      | a wolf        | RoleInfo<role>     | , then "WolfPackList" listing the pack when there's more than one     |
      | a wolf, with a Snow Wolf | RoleInfo<role> | , then "WolfPackSnowList" naming the wolves and the Snow Wolf    |

Feature: Role models

  Scenario: The Wild Child and the Doppelganger pick on the first night only
    # covered by: role_models_are_picked_on_the_first_night_only
    Given the game has a Wild Child and a Doppelganger
    Then on the first night the Wild Child is asked "AskRoleModel" and the Doppelganger "AskDoppelganger"
    And neither is asked again

  Scenario: One who didn't pick gets a random role model
    # covered by: a_random_role_model_is_someone_else
    Given the Doppelganger didn't pick on the first night
    Then a random other player becomes their role model, and they are told "RoleModelChosen"

Feature: Being told about a new role

  Scenario Outline: What the changed player and their new and old teams are told
    # covered by: the_cursed_learns_the_pack_and_the_pack_learns_of_them, the_cursed_turns_and_hears_from_the_pack, a_bitten_cultist_is_missed_by_the_cult, a_convert_meets_the_cult_and_leaves_the_masons, the_apprentice_and_the_beholder_hear_of_the_new_seer, a_doppelganger_who_becomes_a_wolf_meets_the_pack, a_doppelganger_with_no_team_is_told_the_new_role, a_thief_who_steals_into_the_cult_is_introduced, the_last_converts_are_told_nothing, engine_transcripts
    Given <who> <how>
    Then <told>
    Examples:
      | who                | how                              | told                                                                                     |
      | the Cursed         | is attacked by the wolves        | "CursedBitten" and the pack list; each wolf and the Snow Wolf "CursedBittenToWolves"      |
      | a bitten player    | turns at nightfall               | "BittenTurned", and the pack list if there is a pack; old cultmates "CultistBitten", old fellow masons "MasonConverted" |
      | the Traitor        | is left after the last wolf dies | "TraitorTurnWolf"                                                                        |
      | the Snow Wolf      | is the last wolf                 | "SnowWolfTurnWolf"                                                                       |
      | the Apprentice Seer| outlives the Seer                | "ApprenticeNowSeer" naming the Seer; the Beholder "BeholderNewSeer"                       |
      | a player           | is converted to the cult         | "CultConvertYou" and "CultTeam"; the other cultists "CultJoin" with "CultistsList"; old fellow masons "MasonConverted" |
      | the Wild Child     | outlives their role model        | "WildChildTransform" (or "WildChildSnowTransform"); the wolves "WildChildToWolves"        |
      | the Doppelganger   | becomes a role with no team      | the new role's role info; a new Seer's Beholder "BeholderNewSeer"                        |
      | the Doppelganger   | becomes a Mason, wolf, Snow Wolf or Cultist | "DGTransformTo..." naming the role model and the team; the team "DGTo..."     |
      | the Thief          | steals a role                    | the new role's role info; the team "ThiefToMason", "ThiefToWolf", "ThiefToSnowWolf" or "ThiefToCult"; the Beholder "BeholderSeerStolen" |
      | the last cult      | converts everyone at the end     | nothing                                                                                  |

  Scenario: Shooting the Wise Elder is announced
    # covered by: shooting_the_wise_elder_is_announced
    Given the Gunner shoots the Wise Elder
    Then the group is told "GunnerShotWiseElder" and the Gunner becomes a Villager
