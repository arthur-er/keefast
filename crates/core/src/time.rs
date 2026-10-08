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
        let seconds = self.unix_seconds();
        let (days, of_day) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
        let (year, month, day) = civil_from_days(days);
        format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
            of_day / 3600,
            of_day / 60 % 60,
            of_day % 60
        )
    }

    pub(crate) fn decode(text: &str) -> Option<Self> {
        let bytes = BASE64.decode(text.trim()).ok()?;
        Some(Self(i64::from_le_bytes(bytes.try_into().ok()?)))
    }
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
    fn encodes_as_keepass_does() {
        // 2026-10-07 23:15:22 UTC, as KeePassXC wrote it in the corpus.
        let time = Timestamp::decode("islY4g4AAAA=").unwrap();
        assert_eq!(time.unix_seconds(), 1_791_414_922);
        assert_eq!(time.encode(), "islY4g4AAAA=");
        assert_eq!(Timestamp::from_unix_seconds(0).0, UNIX_EPOCH_SECONDS);
    }

    #[test]
    fn converts_system_times() {
        let time = UNIX_EPOCH + std::time::Duration::from_secs(90);
        assert_eq!(Timestamp::from_system_time(time).unix_seconds(), 90);
    }
}
