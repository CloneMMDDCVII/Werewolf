//! Just enough UTC time handling for ban bookkeeping and the legacy
//! `/getban` text, without pulling a date library into the workspace.

use serde::{Deserialize, Serialize};

const SECS_PER_DAY: i64 = 86_400;

/// Seconds since the Unix epoch, UTC. Every timestamp moderation stores
/// is UTC (the C# wrote `DateTime.UtcNow` everywhere it issued a ban).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Timestamp(pub i64);

impl Timestamp {
    /// Builds a timestamp from a UTC calendar date and time.
    pub fn from_utc(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> Self {
        let days = days_from_civil(year, month, day);
        Timestamp(days * SECS_PER_DAY + i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second))
    }

    pub fn plus_seconds(self, seconds: i64) -> Self {
        Timestamp(self.0 + seconds)
    }

    /// The year this timestamp falls in (UTC).
    pub fn year(self) -> i64 {
        civil_from_days(self.0.div_euclid(SECS_PER_DAY)).0
    }

    /// `DateTime.ToString("ddMMMyyyy H:mm:ss zzz").ToUpper()`, the format
    /// `/getban` prints (AdminCommands.cs:139, 152). The offset is always
    /// `+00:00`: the port only stores UTC, where the C# printed whatever
    /// the server's local offset happened to be.
    pub fn legacy_format(self) -> String {
        const MONTHS: [&str; 12] = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];
        let (year, month, day) = civil_from_days(self.0.div_euclid(SECS_PER_DAY));
        let secs = self.0.rem_euclid(SECS_PER_DAY);
        format!(
            "{day:02}{}{year:04} {}:{:02}:{:02} +00:00",
            MONTHS[month as usize - 1],
            secs / 3600,
            (secs % 3600) / 60,
            secs % 60
        )
    }
}

/// Howard Hinnant's `days_from_civil`: days since 1970-01-01.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_round_trips() {
        assert_eq!(Timestamp::from_utc(1970, 1, 1, 0, 0, 0), Timestamp(0));
        assert_eq!(Timestamp(0).legacy_format(), "01JAN1970 0:00:00 +00:00");
    }

    #[test]
    fn formats_like_the_csharp_getban() {
        let t = Timestamp::from_utc(2026, 10, 1, 9, 5, 7);
        assert_eq!(t.legacy_format(), "01OCT2026 9:05:07 +00:00");
        assert_eq!(t.year(), 2026);
    }

    #[test]
    fn handles_leap_days() {
        let t = Timestamp::from_utc(2024, 2, 29, 23, 59, 59);
        assert_eq!(t.legacy_format(), "29FEB2024 23:59:59 +00:00");
        assert_eq!(t.plus_seconds(1).legacy_format(), "01MAR2024 0:00:00 +00:00");
    }
}
