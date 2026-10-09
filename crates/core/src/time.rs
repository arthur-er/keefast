use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

/// Seconds from 0001-01-01 to 1970-01-01, both UTC.
const UNIX_EPOCH_SECONDS: i64 = 62_135_596_800;

/// A point in time as KDBX 4 stores it: whole seconds since 0001-01-01 00:00 UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    pub fn from_unix_seconds(seconds: i64) -> Self {
        Self(seconds + UNIX_EPOCH_SECONDS)
    }

    pub fn from_system_time(time: SystemTime) -> Self {
        let seconds = match time.duration_since(UNIX_EPOCH) {
            Ok(after) => after.as_secs() as i64,
            Err(before) => -(before.duration().as_secs() as i64),
        };
        Self::from_unix_seconds(seconds)
    }

    pub fn unix_seconds(self) -> i64 {
        self.0 - UNIX_EPOCH_SECONDS
    }

    pub(crate) fn from_year_one_seconds(seconds: i64) -> Self {
        Self(seconds)
    }

    /// The KDBX 4 encoding: Base64 of the little-endian seconds.
    pub(crate) fn encode(self) -> String {
        BASE64.encode(self.0.to_le_bytes())
    }

    /// `2026-10-07T23:15:22Z`: UTC in ISO 8601, as KeePassXC reads its Pairing dates.
    pub(crate) fn to_iso_8601(self) -> String {
        let ((year, month, day), (hour, minute, second)) = self.utc();
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
    }

    /// `2026-10-07 23:15:22 UTC`, for people to read.
    pub fn to_utc_string(self) -> String {
        let ((year, month, day), (hour, minute, second)) = self.utc();
        format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC")
    }

    /// The UTC date and time of day.
    fn utc(self) -> ((i64, i64, i64), (i64, i64, i64)) {
        let seconds = self.unix_seconds();
        let (days, of_day) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
        let time = (of_day / 3600, of_day / 60 % 60, of_day % 60);
        (civil_from_days(days), time)
    }

    /// The UTC date, as `2026-10-07`.
    pub fn to_date(self) -> String {
        let (year, month, day) = civil_from_days(self.unix_seconds().div_euclid(86_400));
        format!("{year:04}-{month:02}-{day:02}")
    }

    /// The start of a UTC date written as `2026-10-07`, if there is such a date.
    pub fn parse_date(text: &str) -> Option<Self> {
        let mut parts = text.trim().splitn(3, '-');
        let mut part = |digits: usize| {
            let part = parts.next()?;
            let valid = part.len() == digits && part.bytes().all(|byte| byte.is_ascii_digit());
            valid.then(|| part.parse::<i64>().ok()).flatten()
        };
        let (year, month, day) = (part(4)?, part(2)?, part(2)?);
        let days = days_from_civil(year, month, day);
        // Rejects days a month hasn't got, such as 2026-02-30.
        (civil_from_days(days) == (year, month, day))
            .then(|| Self::from_unix_seconds(days * 86_400))
    }

    /// The expiry for a date typed as `2026-10-07`: `current`, to the second, when it falls on
    /// that date, so an expiry set by another client keeps its time; else the start of the date.
    pub fn expiry_on(date: &str, current: Option<Self>) -> Option<Self> {
        match current {
            Some(current) if current.to_date() == date.trim() => Some(current),
            _ => Self::parse_date(date),
        }
    }

    pub(crate) fn decode(text: &str) -> Option<Self> {
        let bytes = BASE64.decode(text.trim()).ok()?;
        Some(Self(i64::from_le_bytes(bytes.try_into().ok()?)))
    }
}

/// Days from 1970-01-01 to a Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The Gregorian date `days` after 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_iso_8601() {
        let iso = |seconds| Timestamp::from_unix_seconds(seconds).to_iso_8601();
        assert_eq!(iso(1_791_414_922), "2026-10-07T23:15:22Z");
        assert_eq!(iso(951_825_600), "2000-02-29T12:00:00Z");
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn writes_utc_for_people() {
        let time = Timestamp::from_unix_seconds(1_791_414_922);
        assert_eq!(time.to_utc_string(), "2026-10-07 23:15:22 UTC");
    }

    #[test]
    fn encodes_as_keepass_does() {
        // 2026-10-07 23:15:22 UTC, as KeePassXC wrote it in the corpus.
        let time = Timestamp::decode("islY4g4AAAA=").unwrap();
        assert_eq!(time.unix_seconds(), 1_791_414_922);
        assert_eq!(time.encode(), "islY4g4AAAA=");
        assert_eq!(Timestamp::from_unix_seconds(0).0, UNIX_EPOCH_SECONDS);
    }

    #[test]
    fn reads_and_writes_dates() {
        let date = Timestamp::parse_date("2026-10-07").unwrap();
        assert_eq!(date.to_iso_8601(), "2026-10-07T00:00:00Z");
        assert_eq!(date.to_date(), "2026-10-07");
        assert_eq!(
            Timestamp::from_unix_seconds(1_791_414_922).to_date(),
            "2026-10-07"
        );
        let leap = Timestamp::parse_date(" 2000-02-29 ").unwrap();
        assert_eq!(leap.to_date(), "2000-02-29");
        for invalid in [
            "",
            "2026-02-30",
            "2026-13-01",
            "2026-00-10",
            "26-10-07",
            "2026-1-07",
            "2026-10-07x",
            "2026/10/07",
            "+026-10-07",
        ] {
            assert_eq!(Timestamp::parse_date(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn an_expiry_keeps_its_time_on_its_own_date() {
        let current = Timestamp::from_unix_seconds(1_791_414_922);
        assert_eq!(
            Timestamp::expiry_on("2026-10-07", Some(current)),
            Some(current)
        );
        assert_eq!(
            Timestamp::expiry_on("2026-10-08", Some(current)),
            Timestamp::parse_date("2026-10-08")
        );
        assert_eq!(
            Timestamp::expiry_on("2026-10-07", None),
            Timestamp::parse_date("2026-10-07")
        );
        assert_eq!(Timestamp::expiry_on("soon", Some(current)), None);
    }

    #[test]
    fn converts_system_times() {
        let time = UNIX_EPOCH + std::time::Duration::from_secs(90);
        assert_eq!(Timestamp::from_system_time(time).unix_seconds(), 90);
    }
}
