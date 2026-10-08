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

    pub(crate) fn decode(text: &str) -> Option<Self> {
        let bytes = BASE64.decode(text.trim()).ok()?;
        Some(Self(i64::from_le_bytes(bytes.try_into().ok()?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
