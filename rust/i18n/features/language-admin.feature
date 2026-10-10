# Scope: language-file admin tools (/validatelangs, /uploadlang)
# Status: approved by the coordinator under Pierre's delegated approval,
# 10 Oct 2026, with the instruction to follow the C#.
#
# Legacy sources (Werewolf for Telegram/Werewolf Control/):
#   Commands/AdminCommands.cs:102-129 (uploadlang), 161-218 (validatelangs)
#   Helpers/LanguageHelper.cs:64-178 (ValidateFiles, ValidateLanguageFile),
#     180-269 (UploadFile), 271-430 (UseNewLanguageFile), 502-668 (checks, reports)
#   Handlers/UpdateHandler.cs:1011-1019, 1263-1298 (validate/upload callbacks)
#
# Lands in the i18n crate (`i18n::admin`), pure over files on disk. The
# Telegram download, the base/variant menus and the buttons are the
# presenter's. Dropped as infra: the git commit of English.xml from the bot,
# the Language DB table, and broadcasting reloads to nodes (one process now:
# the caller reloads its Catalog).
#
# Deliberate changes:
#   - Every duplicated key is flagged, not just CultConvertSerialKiller and
#     CupidChosen; it's the same bug ("the second one is never used").
#   - The all-files summary checks ads per file (the C# checked the whole
#     run, so one file with ads hid every later file's details).
#   - Checks run on the raw value, markup included, so a link hidden in an
#     href is caught too.
#   - Deprecated English strings are skipped entirely (the C# still checked
#     placeholders in them when present).

Feature: Checking a language file against English.xml

  Scenario: Missing language node attributes are fatal
    # covered by: missing_node_attributes_are_fatal
    Given a file whose <language> has no variant
    Then it has the fatal error "Variant is missing" under "*Language Node*"

  Scenario: Base and variant must fit Telegram's 64-byte callback data
    # not tested: superseded, admin buttons now carry short ids so long names are allowed (long_names_are_fine_now_that_buttons_carry_ids, admin_buttons_round_trip_and_stay_short)
    Given "setlang|-1001049529775|<base>|<variant>|v" is over 64 UTF-8 bytes
    Then it has the fatal error "Base and variant are too long. (*38 utf8 byte max*)"
    And the same for "preferred|-1001049529775|<file name>|toggle": "Filename is too long. (*32 utf8 byte max*)"

  Scenario: A string English has but the file lacks
    # covered by: missing_strings_skip_deprecated_ones
    Then it is a missing string, unless English marks it deprecated

  Scenario: Placeholders must match English
    # covered by: placeholders_must_match_english
    Given English "PlayerJoined" uses {0}
    When a value lacks {0}, that is the error "Missing {0}"
    And when a value uses {1}, that is the error "Extra {1}"

  Scenario: GIF captions over 1000 characters are fatal
    # covered by: long_gif_captions_are_fatal
  Scenario: A value mentioning /join is flagged once per key
    # covered by: join_is_flagged_once_per_key
  Scenario: Invite links and foreign @usernames are ads
    # covered by: ads_are_invite_links_and_foreign_mentions
    Then "t.me/+abc", "telegram.me/joinchat/abc" and " @someone" are ads
    But " @werewolfbot", " @werewolfsupport", " @para949" (and the other house accounts) are not
  Scenario: A duplicated key is a warning
    # covered by: duplicated_keys_are_warnings

Feature: /validatelangs (language admins and global admins)

  Scenario: The admin picks a base, then a variant or "All"
    # covered by: bases_are_listed_once_and_sorted, language_admins_validate_installed_files
    Then the menu lists every base once, sorted, two per row
  Scenario: Validating one file
    # covered by: file_report_lists_problems_by_kind, file_report_shows_only_ads_when_there_are_any
    Then the report lists ads, or else errors, missing values, duplicates, /join strings and fatal errors
    And ends "*Validation complete*.\nErrors: n\nMissing strings: m"
  Scenario: Validating every variant of a base
    # covered by: summary_reports_each_file_with_problems, language_admins_validate_installed_files
    Then each file with problems gets a section, and a summary names the most and least recently updated files

Feature: /uploadlang (language admins and global admins)

  Scenario: Must reply to a document
    # covered by: upload_needs_an_xml_document, language_admins_upload_and_install_a_file
    Then the reply is "Please reply to the file with /uploadlang"
  Scenario: Must be XML
    # covered by: upload_needs_an_xml_document
    Then the reply is "The file must be an XML file! (*.xml)"
  Scenario: A file that clashes with an existing language is fatal
    # covered by: uploads_that_clash_with_an_installed_language_are_fatal
    When the file name matches but the language name differs,
    Or the name matches but base or variant differ,
    Or base and variant match but the file name differs
    Then it is fatal and names the clashing file
  Scenario: Comparing with the file it replaces
    # covered by: uploading_a_clean_update_compares_with_the_old_file, uploading_a_new_language_asks_to_double_check
    Then the report shows the new file's problems and the old file's error counts,
    And flags an isDefault change, or for a new language asks to double-check name, base and variant
  Scenario: Fatal errors or ads block the upload
    # covered by: ads_block_an_upload, uploads_that_clash_with_an_installed_language_are_fatal, language_admins_upload_and_install_a_file
    Then the reply is "Fatal errors present, cannot upload."
    Otherwise the admin chooses "New" or "Old"
  Scenario: Keeping the old file
    # not tested: gap
    Then the reply is "No action taken."
  Scenario: Installing the new file
    # covered by: installing_replaces_the_file_with_the_same_language_name, installing_a_new_language_adds_its_file, installing_over_another_languages_base_and_variant_is_refused, language_admins_upload_and_install_a_file
    Then it replaces the file with the same language name,
    Or is refused when another language already has its base and variant,
    And the caller reloads the language catalog
