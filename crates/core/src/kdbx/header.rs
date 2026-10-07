//! The binary headers of a Vault file.

use crate::OpenError;

const SIGNATURE: [u8; 4] = [0x03, 0xd9, 0xa2, 0x9a];
const KEEPASS_1: u32 = 0xb54b_fb65;
const KDBX_PRE_RELEASE: u32 = 0xb54b_fb66;
const KDBX: u32 = 0xb54b_fb67;

/// Header field IDs. KDBX 3.1 and 4 share the numbering.
pub(crate) mod field {
    pub(crate) const END: u8 = 0;
    pub(crate) const CIPHER: u8 = 2;
    pub(crate) const COMPRESSION: u8 = 3;
    pub(crate) const MASTER_SEED: u8 = 4;
    pub(crate) const TRANSFORM_SEED: u8 = 5;
    pub(crate) const TRANSFORM_ROUNDS: u8 = 6;
    pub(crate) const ENCRYPTION_IV: u8 = 7;
    pub(crate) const PROTECTED_STREAM_KEY: u8 = 8;
    pub(crate) const STREAM_START_BYTES: u8 = 9;
    pub(crate) const INNER_RANDOM_STREAM: u8 = 10;
    pub(crate) const KDF_PARAMETERS: u8 = 11;
    pub(crate) const PUBLIC_CUSTOM_DATA: u8 = 12;
}

/// Inner header field IDs (KDBX 4).
pub(crate) mod inner_field {
    pub(crate) const END: u8 = 0;
    pub(crate) const RANDOM_STREAM_ID: u8 = 1;
    pub(crate) const RANDOM_STREAM_KEY: u8 = 2;
    pub(crate) const BINARY: u8 = 3;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Version {
    Kdbx3 { minor: u16 },
    Kdbx4 { minor: u16 },
}

pub(crate) fn version(data: &[u8]) -> Result<Version, OpenError> {
    let not_a_vault = || OpenError::Corrupt("this is not a KeePass Vault file".into());
    let signature = data.get(..4).ok_or_else(not_a_vault)?;
    if signature != SIGNATURE {
        return Err(not_a_vault());
    }
    let read = |range: std::ops::Range<usize>| data.get(range).ok_or_else(not_a_vault);
    let kind = u32::from_le_bytes(read(4..8)?.try_into().expect("4 bytes"));
    let minor = u16::from_le_bytes(read(8..10)?.try_into().expect("2 bytes"));
    let major = u16::from_le_bytes(read(10..12)?.try_into().expect("2 bytes"));
    match (kind, major) {
        (KEEPASS_1, _) => Err(OpenError::Unsupported("KeePass 1 (KDB)".into())),
        (KDBX_PRE_RELEASE, _) => Err(OpenError::Unsupported("KDBX 2 pre-release".into())),
        (KDBX, 3) => Ok(Version::Kdbx3 { minor }),
        (KDBX, 4) => Ok(Version::Kdbx4 { minor }),
        (KDBX, _) => Err(OpenError::Unsupported(format!("KDBX {major}.{minor}"))),
        _ => Err(not_a_vault()),
    }
}

pub(crate) fn write_version(out: &mut Vec<u8>, minor: u16) {
    out.extend_from_slice(&SIGNATURE);
    out.extend_from_slice(&KDBX.to_le_bytes());
    out.extend_from_slice(&minor.to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
}

/// A header field's ID and value.
pub(crate) type Field = (u8, Vec<u8>);

/// Reads type-length-value header fields up to and including the end field. KDBX 3.1 has
/// 16-bit lengths, KDBX 4 32-bit ones. Returns the fields and where the header ends.
pub(crate) fn read_fields(
    data: &[u8],
    start: usize,
    wide_lengths: bool,
) -> Result<(Vec<Field>, usize), OpenError> {
    let truncated = || OpenError::Corrupt("the header is truncated".into());
    let mut fields = Vec::new();
    let mut pos = start;
    loop {
        let id = *data.get(pos).ok_or_else(truncated)?;
        let (len, len_size) = if wide_lengths {
            let bytes = data.get(pos + 1..pos + 5).ok_or_else(truncated)?;
            (
                u32::from_le_bytes(bytes.try_into().expect("4 bytes")) as usize,
                4,
            )
        } else {
            let bytes = data.get(pos + 1..pos + 3).ok_or_else(truncated)?;
            (
                u16::from_le_bytes(bytes.try_into().expect("2 bytes")) as usize,
                2,
            )
        };
        let value_start = pos + 1 + len_size;
        let value = data
            .get(value_start..value_start + len)
            .ok_or_else(truncated)?;
        pos = value_start + len;
        if id == field::END {
            return Ok((fields, pos));
        }
        fields.push((id, value.to_vec()));
    }
}

/// Writes a KDBX 4 type-length-value field.
pub(crate) fn write_field(out: &mut Vec<u8>, id: u8, value: &[u8]) {
    out.push(id);
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value);
}

/// KDBX 4's typed key-value map, used for KDF parameters and public custom data. Entries keep
/// their order and their raw bytes, so a dictionary is written back as it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VariantDictionary {
    version: u16,
    entries: Vec<VariantEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VariantEntry {
    kind: u8,
    key: String,
    value: Vec<u8>,
}

const VARIANT_UINT32: u8 = 0x04;
const VARIANT_UINT64: u8 = 0x05;
const VARIANT_BYTES: u8 = 0x42;

impl VariantDictionary {
    pub(crate) fn new() -> Self {
        Self {
            version: 0x0100,
            entries: Vec::new(),
        }
    }

    pub(crate) fn parse(data: &[u8]) -> Result<Self, OpenError> {
        let truncated = || OpenError::Corrupt("the KDF parameters are truncated".into());
        let take = |pos: &mut usize, len: usize| -> Result<&[u8], OpenError> {
            let bytes = data.get(*pos..*pos + len).ok_or_else(truncated)?;
            *pos += len;
            Ok(bytes)
        };
        let mut pos = 0;
        let version = u16::from_le_bytes(take(&mut pos, 2)?.try_into().expect("2 bytes"));
        if version >> 8 != 1 {
            return Err(OpenError::Unsupported(format!(
                "variant dictionary version {version:#06x}"
            )));
        }
        let mut entries = Vec::new();
        loop {
            let kind = take(&mut pos, 1)?[0];
            if kind == 0 {
                break;
            }
            let key_len = u32::from_le_bytes(take(&mut pos, 4)?.try_into().expect("4 bytes"));
            let key = String::from_utf8(take(&mut pos, key_len as usize)?.to_vec())
                .map_err(|_| OpenError::Corrupt("a KDF parameter name is not UTF-8".into()))?;
            let value_len = u32::from_le_bytes(take(&mut pos, 4)?.try_into().expect("4 bytes"));
            let value = take(&mut pos, value_len as usize)?.to_vec();
            entries.push(VariantEntry { kind, key, value });
        }
        Ok(Self { version, entries })
    }

    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.version.to_le_bytes().to_vec();
        for entry in &self.entries {
            out.push(entry.kind);
            out.extend_from_slice(&(entry.key.len() as u32).to_le_bytes());
            out.extend_from_slice(entry.key.as_bytes());
            out.extend_from_slice(&(entry.value.len() as u32).to_le_bytes());
            out.extend_from_slice(&entry.value);
        }
        out.push(0);
        out
    }

    fn get(&self, key: &str, kind: u8) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|entry| entry.key == key && entry.kind == kind)
            .map(|entry| entry.value.as_slice())
    }

    pub(crate) fn bytes(&self, key: &str) -> Option<&[u8]> {
        self.get(key, VARIANT_BYTES)
    }

    pub(crate) fn u32(&self, key: &str) -> Option<u32> {
        Some(u32::from_le_bytes(
            self.get(key, VARIANT_UINT32)?.try_into().ok()?,
        ))
    }

    pub(crate) fn u64(&self, key: &str) -> Option<u64> {
        Some(u64::from_le_bytes(
            self.get(key, VARIANT_UINT64)?.try_into().ok()?,
        ))
    }

    fn set(&mut self, key: &str, kind: u8, value: Vec<u8>) {
        match self.entries.iter_mut().find(|entry| entry.key == key) {
            Some(entry) => {
                entry.kind = kind;
                entry.value = value;
            }
            None => self.entries.push(VariantEntry {
                kind,
                key: key.into(),
                value,
            }),
        }
    }

    pub(crate) fn set_bytes(&mut self, key: &str, value: &[u8]) {
        self.set(key, VARIANT_BYTES, value.to_vec());
    }

    pub(crate) fn set_u64(&mut self, key: &str, value: u64) {
        self.set(key, VARIANT_UINT64, value.to_le_bytes().to_vec());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_dictionaries_round_trip_including_unknown_types() {
        let mut dictionary = VariantDictionary::new();
        dictionary.set_bytes("S", &[1, 2, 3]);
        dictionary.set_u64("R", 6_000);
        dictionary.set("Flag", 0x08, vec![1]);

        let parsed = VariantDictionary::parse(&dictionary.to_bytes()).unwrap();

        assert_eq!(parsed, dictionary);
        assert_eq!(parsed.bytes("S"), Some(&[1, 2, 3][..]));
        assert_eq!(parsed.u64("R"), Some(6_000));
        assert_eq!(parsed.u32("R"), None);
    }

    #[test]
    fn truncated_variant_dictionaries_are_corrupt() {
        let mut dictionary = VariantDictionary::new();
        dictionary.set_u64("R", 6_000);
        let bytes = dictionary.to_bytes();

        let result = VariantDictionary::parse(&bytes[..bytes.len() - 3]);

        assert!(matches!(result, Err(OpenError::Corrupt(_))));
    }
}
