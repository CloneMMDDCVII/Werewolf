//! `/restore` (AdminCommands.cs:496-581): moving a player's history from
//! an old Telegram account to a new one. The check scores how likely the
//! two accounts belong to the same person and asks for confirmation; the
//! move itself (UpdateHandler.cs:1125-1152, werewolf.sql `RestoreAccount`)
//! is `db::Games::move_player_history`. Achievements, which the C# also
//! merged, are dropped in this port.

/// What the check needs to know about one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountFacts {
    pub telegram_id: i64,
    pub name: String,
    pub username: Option<String>,
    pub language: Option<String>,
    /// Ids of the games played, ascending.
    pub game_ids: Vec<i64>,
    /// Distinct groups those games were played in.
    pub group_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreCheck {
    /// "usage: /restore <oldid> <newid>"
    Usage,
    NotFound,
    OldAccountBanned,
    /// The report to show, ending with a yes/no question. A dev confirms
    /// with `restore|<old>|<new>` (UpdateHandler.cs:986-1001).
    Ready {
        old: i64,
        new: i64,
        report: String,
        score: i64,
    },
}

impl RestoreCheck {
    pub fn reply(&self) -> &str {
        match self {
            RestoreCheck::Usage => "usage: /restore <oldid> <newid>",
            RestoreCheck::NotFound => "Account not found in database",
            RestoreCheck::OldAccountBanned => "Old account was global banned!",
            RestoreCheck::Ready { report, .. } => report,
        }
    }
}

/// The two ids `/restore` takes, or `None` for the usage message.
pub fn parse_restore_args(argument: Option<&str>) -> Option<(i64, i64)> {
    let mut words = argument?.split_whitespace();
    Some((words.next()?.parse().ok()?, words.next()?.parse().ok()?))
}

/// Scores the pair starting from 100 (AdminCommands.cs:527-578).
///
/// Differences from the C#: there are no database row ids to compare, so
/// "old account is newer" only compares Telegram ids; accounts with no
/// games skip the overlap check instead of crashing; and a new account
/// with no groups counts as 0% shared instead of dividing by zero.
pub fn restore_report(old: &AccountFacts, new: &AccountFacts, old_is_banned: bool) -> RestoreCheck {
    if old_is_banned {
        return RestoreCheck::OldAccountBanned;
    }
    let mut score: i64 = 100;
    let mut report = String::new();

    if old.telegram_id > new.telegram_id {
        score -= 30;
        report += "Old account given is newer than new account\n";
    }
    if let (Some(old_last), Some(new_first)) = (old.game_ids.iter().max(), new.game_ids.iter().min()) {
        if old_last > new_first {
            score -= 30;
            report += "Account games overlap - old account has played a game since new account started\n";
        }
    }

    let shared = new.group_ids.iter().filter(|g| old.group_ids.contains(g)).count() as i64;
    let group_like = if new.group_ids.is_empty() {
        0
    } else {
        shared * 100 / new.group_ids.len() as i64
    };
    score -= 20 - group_like * 20 / 100;
    report += &format!("Percent of new groups that were in old account: {group_like}%\n");

    let username_distance = levenshtein(
        old.username.as_deref().unwrap_or(""),
        new.username.as_deref().unwrap_or(""),
    ) as i64;
    if username_distance != 0 {
        let name_distance = levenshtein(&old.name, &new.name) as i64;
        score -= (username_distance + name_distance) / 2;
        report +=
            &format!("\nLevenshtein Distance:\nUsernames: {username_distance}\nDisplay name: {name_distance}\n\n");
    }

    if old.language != new.language {
        score -= 10;
        report += "Account languages are set differently\n";
    }

    report += &format!("Accuracy score: {score}%\n\nDo you want to restore the account?");
    RestoreCheck::Ready {
        old: old.telegram_id,
        new: new.telegram_id,
        report,
        score,
    }
}

/// The reply once a dev confirms (UpdateHandler.cs:1151).
pub fn restored_reply(old_name: &str, new_name: &str) -> String {
    format!("Restored stats from {old_name} to {new_name}")
}

/// Edit distance between two strings, by character (`ComputeLevenshtein`,
/// Commands/Helpers.cs:375-410).
pub fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (above + 1).min(row[j] + 1).min(diagonal + usize::from(ca != *cb));
            diagonal = above;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: i64, name: &str, username: Option<&str>, games: &[i64], groups: &[i64]) -> AccountFacts {
        AccountFacts {
            telegram_id: id,
            name: name.into(),
            username: username.map(Into::into),
            language: Some("English".into()),
            game_ids: games.to_vec(),
            group_ids: groups.to_vec(),
        }
    }

    fn score(check: &RestoreCheck) -> i64 {
        match check {
            RestoreCheck::Ready { score, .. } => *score,
            other => panic!("expected a report, got {other:?}"),
        }
    }

    #[test]
    fn parses_two_ids_or_asks_for_usage() {
        assert_eq!(parse_restore_args(Some("5 9")), Some((5, 9)));
        assert_eq!(parse_restore_args(Some("5")), None);
        assert_eq!(parse_restore_args(Some("5 x")), None);
        assert_eq!(parse_restore_args(None), None);
        assert_eq!(RestoreCheck::Usage.reply(), "usage: /restore <oldid> <newid>");
    }

    #[test]
    fn a_clean_move_scores_100() {
        let old = account(5, "Paul", Some("paul"), &[1, 2], &[100, 200]);
        let new = account(9, "Paul", Some("paul"), &[3, 4], &[100, 200]);
        let check = restore_report(&old, &new, false);
        assert_eq!(score(&check), 100);
        assert_eq!(
            check.reply(),
            "Percent of new groups that were in old account: 100%\nAccuracy score: 100%\n\nDo you want to restore the account?"
        );
    }

    #[test]
    fn every_warning_costs_its_points() {
        let mut old = account(9, "Paul", Some("paul"), &[1, 5], &[100]);
        old.language = Some("Deutsch".into());
        let new = account(5, "Pauline", Some("pauly"), &[3], &[100, 200]);
        let check = restore_report(&old, &new, false);
        // 100 - 30 (newer) - 30 (overlap) - 10 (50% groups) - (1 + 3) / 2 - 10 (language)
        assert_eq!(score(&check), 18);
        let text = check.reply();
        assert!(text.contains("Old account given is newer than new account\n"));
        assert!(text.contains("Account games overlap"));
        assert!(text.contains("Percent of new groups that were in old account: 50%"));
        assert!(text.contains("Usernames: 1\nDisplay name: 3\n"));
        assert!(text.contains("Account languages are set differently\n"));
    }

    #[test]
    fn a_banned_old_account_stops_the_check() {
        let old = account(5, "Paul", None, &[], &[]);
        assert_eq!(restore_report(&old, &old, true), RestoreCheck::OldAccountBanned);
    }

    #[test]
    fn accounts_without_games_do_not_crash_the_check() {
        let old = account(5, "Paul", Some("paul"), &[], &[]);
        let new = account(9, "Paul", Some("paul"), &[], &[]);
        assert_eq!(score(&restore_report(&old, &new, false)), 80);
    }

    #[test]
    fn levenshtein_matches_the_textbook_cases() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("flaw", "lawn"), 2);
        assert_eq!(levenshtein("Zoë", "Zoe"), 1);
    }
}
