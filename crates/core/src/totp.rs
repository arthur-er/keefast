//! TOTP (RFC 6238): one-time codes from a secret and the time, and the Entry fields KeePass
//! clients keep TOTP settings in.
//!
//! Every client's fields are read: KeePassXC's and KeeWeb's `otp` field (an `otpauth://` URI, the
//! KeeOtp plugin's `key=…` settings, or KeeWeb's bare secret), KeePassXC's older `TOTP Seed` and
//! `TOTP Settings`, and KeePass2's `TimeOtp-*` fields. A change is written back to the formats
//! the Entry already uses, so each client keeps reading it, and to an `otpauth://` URI otherwise.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use hmac::{Hmac, KeyInit, Mac};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use secrecy::SecretString;
use sha1::Sha1;
use sha2::{Sha256, Sha512};
use url::Url;
use zeroize::Zeroizing;

use crate::Timestamp;

/// The hash a TOTP's codes are computed with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TotpAlgorithm {
    #[default]
    Sha1,
    Sha256,
    Sha512,
}

/// Why TOTP settings could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TotpError {
    #[error("the TOTP secret is missing")]
    MissingSecret,
    #[error("the TOTP secret is not valid Base32")]
    InvalidSecret,
    #[error("a TOTP code has 1 to 10 digits")]
    InvalidDigits,
    #[error("a TOTP period is 1 to 86400 seconds")]
    InvalidPeriod,
    #[error("not an otpauth://totp/ URI")]
    NotTotp,
}

/// An Entry's TOTP settings: everything needed to compute its codes.
#[derive(Clone)]
pub struct Totp {
    secret: Zeroizing<Vec<u8>>,
    algorithm: TotpAlgorithm,
    digits: u32,
    period: u32,
    /// Codes in Steam's alphabet rather than decimal digits, as KeePassXC's Steam encoder.
    steam: bool,
    /// Who the TOTP is for, as an `otpauth://` URI's label names them.
    issuer: Option<String>,
    account: Option<String>,
}

/// The code for one moment, with how long it stays valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TotpCode {
    code: String,
    remaining: u32,
    period: u32,
}

impl TotpCode {
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Seconds until the next code, from 1 to the period.
    pub fn remaining(&self) -> u32 {
        self.remaining
    }

    pub fn period(&self) -> u32 {
        self.period
    }
}

const DEFAULT_DIGITS: u32 = 6;
const DEFAULT_PERIOD: u32 = 30;
const STEAM_DIGITS: u32 = 5;
const MAX_DIGITS: u32 = 10;
const MAX_PERIOD: u32 = 86_400;
const STEAM_ALPHABET: &[u8] = b"23456789BCDFGHJKMNPQRTVWXY";
const BASE32_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
/// What `otpauth://` URIs leave unescaped: RFC 3986's unreserved characters, as KeePassXC.
const URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

impl Totp {
    pub const DEFAULT_DIGITS: u32 = DEFAULT_DIGITS;
    pub const DEFAULT_PERIOD: u32 = DEFAULT_PERIOD;

    /// TOTP settings by hand: a Base32 secret, which may be lowercase and hold spaces and
    /// dashes, and the code's hash, length and period in seconds.
    pub fn new(
        secret: &str,
        algorithm: TotpAlgorithm,
        digits: u32,
        period: u32,
    ) -> Result<Self, TotpError> {
        Self::build(decode_base32(secret)?, algorithm, digits, period)
    }

    /// TOTP settings from an `otpauth://totp/` URI, as sites show them in QR codes.
    pub fn from_uri(uri: &str) -> Result<Self, TotpError> {
        let url = Url::parse(uri.trim()).map_err(|_| TotpError::NotTotp)?;
        let is_totp = url.scheme() == "otpauth"
            && url
                .host_str()
                .is_some_and(|kind| kind.eq_ignore_ascii_case("totp"));
        if !is_totp {
            return Err(TotpError::NotTotp);
        }
        let query = |name: &str| {
            url.query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| Zeroizing::new(value.into_owned()))
        };
        let secret = query("secret").ok_or(TotpError::MissingSecret)?;
        let steam = query("encoder").is_some_and(|encoder| encoder.as_str() == "steam");
        let default_digits = if steam { STEAM_DIGITS } else { DEFAULT_DIGITS };
        let algorithm = query("algorithm").map_or(TotpAlgorithm::Sha1, |name| algorithm(&name));
        let digits = number(
            query("digits").as_deref().map(|s| s.as_str()),
            default_digits,
        )
        .ok_or(TotpError::InvalidDigits)?;
        let period = number(
            query("period").as_deref().map(|s| s.as_str()),
            DEFAULT_PERIOD,
        )
        .ok_or(TotpError::InvalidPeriod)?;

        let mut totp = Self::build(decode_base32(&secret)?, algorithm, digits, period)?;
        totp.steam = steam;
        let label = percent_decode_str(url.path().trim_start_matches('/')).decode_utf8_lossy();
        let (label_issuer, account) = match label.split_once(':') {
            Some((issuer, account)) => (Some(issuer.trim()), account.trim()),
            None => (None, label.trim()),
        };
        totp.issuer = query("issuer")
            .map(|issuer| issuer.as_str().to_owned())
            .or(label_issuer.map(str::to_owned))
            .filter(|issuer| !issuer.is_empty());
        totp.account = Some(account.to_owned()).filter(|account| !account.is_empty());
        Ok(totp)
    }

    /// TOTP settings from what the user pastes or types: an `otpauth://` URI, or a Base32
    /// secret alone, which gets the usual 6 digits every 30 seconds with SHA-1.
    pub fn parse(text: &str) -> Result<Self, TotpError> {
        let text = text.trim();
        if is_otpauth(text) {
            Self::from_uri(text)
        } else {
            Self::new(text, TotpAlgorithm::Sha1, DEFAULT_DIGITS, DEFAULT_PERIOD)
        }
    }

    fn build(
        secret: Zeroizing<Vec<u8>>,
        algorithm: TotpAlgorithm,
        digits: u32,
        period: u32,
    ) -> Result<Self, TotpError> {
        if !(1..=MAX_DIGITS).contains(&digits) {
            return Err(TotpError::InvalidDigits);
        }
        if !(1..=MAX_PERIOD).contains(&period) {
            return Err(TotpError::InvalidPeriod);
        }
        Ok(Self {
            secret,
            algorithm,
            digits,
            period,
            steam: false,
            issuer: None,
            account: None,
        })
    }

    pub fn algorithm(&self) -> TotpAlgorithm {
        self.algorithm
    }

    pub fn digits(&self) -> u32 {
        self.digits
    }

    /// Seconds each code is valid for.
    pub fn period(&self) -> u32 {
        self.period
    }

    /// Whether codes are in Steam's alphabet rather than decimal digits.
    pub fn is_steam(&self) -> bool {
        self.steam
    }

    /// The secret in Base32, without padding.
    pub fn secret(&self) -> SecretString {
        SecretString::from(encode_base32(&self.secret, false).as_str())
    }

    /// The settings as an `otpauth://` URI, as KeePassXC writes them. The URI's own label is
    /// kept; without one, the label names the Entry's title and username.
    pub fn to_uri(&self, title: &str, username: &str) -> SecretString {
        let or = |value: &Option<String>, fallback: &str, empty: &str| {
            let value = value.as_deref().unwrap_or(fallback);
            let value = if value.is_empty() { empty } else { value };
            utf8_percent_encode(value, URI_COMPONENT).to_string()
        };
        let issuer = or(&self.issuer, title, "keefast");
        let account = or(&self.account, username, "none");
        let mut uri = format!(
            "otpauth://totp/{issuer}:{account}?secret={}&period={}&digits={}&issuer={issuer}",
            encode_base32(&self.secret, false).as_str(),
            self.period,
            self.digits,
        );
        if self.steam {
            uri.push_str("&encoder=steam");
        }
        if self.algorithm != TotpAlgorithm::Sha1 {
            uri.push_str("&algorithm=");
            uri.push_str(algorithm_name(self.algorithm));
        }
        SecretString::from(uri)
    }

    /// The code at `time`.
    pub fn code(&self, time: Timestamp) -> TotpCode {
        let seconds = u64::try_from(time.unix_seconds()).unwrap_or(0);
        let period = u64::from(self.period);
        let counter = (seconds / period).to_be_bytes();
        let hash = match self.algorithm {
            TotpAlgorithm::Sha1 => hmac::<Hmac<Sha1>>(&self.secret, &counter),
            TotpAlgorithm::Sha256 => hmac::<Hmac<Sha256>>(&self.secret, &counter),
            TotpAlgorithm::Sha512 => hmac::<Hmac<Sha512>>(&self.secret, &counter),
        };
        // RFC 4226's dynamic truncation.
        let offset = usize::from(hash[hash.len() - 1] & 0xf);
        let bytes = hash[offset..offset + 4].try_into().expect("four bytes");
        let mut value = u64::from(u32::from_be_bytes(bytes) & 0x7fff_ffff);

        let code = if self.steam {
            // KeePassXC's Steam encoder: least significant character first.
            (0..self.digits)
                .map(|_| {
                    let char = STEAM_ALPHABET[(value % STEAM_ALPHABET.len() as u64) as usize];
                    value /= STEAM_ALPHABET.len() as u64;
                    char::from(char)
                })
                .collect()
        } else {
            let digits = self.digits as usize;
            format!("{:0digits$}", value % 10u64.pow(self.digits))
        };
        TotpCode {
            code,
            remaining: (period - seconds % period) as u32,
            period: self.period,
        }
    }
}

/// Two TOTPs are equal when they give the same codes, whatever their labels.
impl PartialEq for Totp {
    fn eq(&self, other: &Self) -> bool {
        self.secret == other.secret
            && self.algorithm == other.algorithm
            && self.digits == other.digits
            && self.period == other.period
            && self.steam == other.steam
    }
}

impl Eq for Totp {}

impl fmt::Debug for Totp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Totp")
            .field("secret", &"[hidden]")
            .field("algorithm", &self.algorithm)
            .field("digits", &self.digits)
            .field("period", &self.period)
            .field("steam", &self.steam)
            .finish_non_exhaustive()
    }
}

fn hmac<M: Mac + KeyInit>(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac = <M as KeyInit>::new_from_slice(key).expect("HMAC takes keys of any length");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

fn is_otpauth(text: &str) -> bool {
    text.get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("otpauth:"))
}

/// A hash by any name KeePass clients give it, SHA-1 when unknown, as KeePassXC reads them.
fn algorithm(name: &str) -> TotpAlgorithm {
    match name.trim().to_ascii_uppercase().as_str() {
        "SHA256" | "SHA-256" | "HMAC-SHA-256" => TotpAlgorithm::Sha256,
        "SHA512" | "SHA-512" | "HMAC-SHA-512" => TotpAlgorithm::Sha512,
        _ => TotpAlgorithm::Sha1,
    }
}

fn algorithm_name(algorithm: TotpAlgorithm) -> &'static str {
    match algorithm {
        TotpAlgorithm::Sha1 => "SHA1",
        TotpAlgorithm::Sha256 => "SHA256",
        TotpAlgorithm::Sha512 => "SHA512",
    }
}

/// A number setting: `default` when absent, `None` when not a number.
fn number(text: Option<&str>, default: u32) -> Option<u32> {
    match text.map(str::trim) {
        None | Some("") => Some(default),
        Some(text) => text.parse().ok(),
    }
}

/// Decodes Base32 leniently, as people copy secrets: any case, with spaces, dashes and
/// padding or without.
fn decode_base32(text: &str) -> Result<Zeroizing<Vec<u8>>, TotpError> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(text.len() * 5 / 8));
    let (mut buffer, mut bits) = (0u64, 0u32);
    for char in text.bytes() {
        if char.is_ascii_whitespace() || char == b'-' || char == b'=' {
            continue;
        }
        let value = BASE32_ALPHABET
            .iter()
            .position(|&c| c == char.to_ascii_uppercase())
            .ok_or(TotpError::InvalidSecret)?;
        buffer = (buffer << 5) | value as u64;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    if bytes.is_empty() {
        return Err(TotpError::MissingSecret);
    }
    Ok(bytes)
}

fn encode_base32(bytes: &[u8], pad: bool) -> Zeroizing<String> {
    let mut text = Zeroizing::new(String::with_capacity(bytes.len().div_ceil(5) * 8));
    for chunk in bytes.chunks(5) {
        let mut block = [0u8; 5];
        block[..chunk.len()].copy_from_slice(chunk);
        let value = block
            .iter()
            .fold(0u64, |value, &byte| value << 8 | u64::from(byte));
        let chars = (chunk.len() * 8).div_ceil(5);
        for index in 0..8 {
            if index < chars {
                let digit = (value >> (35 - 5 * index)) & 0x1f;
                text.push(char::from(BASE32_ALPHABET[digit as usize]));
            } else if pad {
                text.push('=');
            }
        }
    }
    text
}

// Where KeePass clients keep TOTP settings in an Entry's fields.

/// KeePassXC's and KeeWeb's field: an `otpauth://` URI, KeeOtp settings or a bare secret.
const OTP: &str = "otp";
/// KeePassXC's older fields: the Base32 secret, and `period;digits` or `period;S` for Steam.
const LEGACY_SEED: &str = "TOTP Seed";
const LEGACY_SETTINGS: &str = "TOTP Settings";
/// KeePass2's fields. The secret is in one of four encodings.
const KEEPASS2_BASE32: &str = "TimeOtp-Secret-Base32";
const KEEPASS2_SECRETS: [&str; 4] = [
    KEEPASS2_BASE32,
    "TimeOtp-Secret",
    "TimeOtp-Secret-Hex",
    "TimeOtp-Secret-Base64",
];
const KEEPASS2_LENGTH: &str = "TimeOtp-Length";
const KEEPASS2_PERIOD: &str = "TimeOtp-Period";
const KEEPASS2_ALGORITHM: &str = "TimeOtp-Algorithm";

/// Every field TOTP settings are kept in.
pub(crate) const TOTP_FIELDS: [&str; 10] = [
    OTP,
    LEGACY_SEED,
    LEGACY_SETTINGS,
    KEEPASS2_SECRETS[0],
    KEEPASS2_SECRETS[1],
    KEEPASS2_SECRETS[2],
    KEEPASS2_SECRETS[3],
    KEEPASS2_LENGTH,
    KEEPASS2_PERIOD,
    KEEPASS2_ALGORITHM,
];

/// A field's value by name; `None` when the field is missing or empty.
pub(crate) type Fields<'a> = &'a dyn Fn(&str) -> Option<Zeroizing<String>>;

/// The Entry's TOTP settings, if it has any. Formats are tried in KeePassXC's order.
pub(crate) fn read(field: Fields) -> Option<Result<Totp, TotpError>> {
    if let Some(seed) = field(LEGACY_SEED) {
        return Some(read_legacy(
            &seed,
            field(LEGACY_SETTINGS).as_deref().map(|s| s.as_str()),
        ));
    }
    if let Some(otp) = field(OTP) {
        return Some(read_otp(&otp));
    }
    let secret = KEEPASS2_SECRETS
        .iter()
        .find_map(|&name| Some((name, field(name)?)))?;
    Some(read_keepass2(secret, field))
}

fn read_legacy(seed: &str, settings: Option<&str>) -> Result<Totp, TotpError> {
    let (period, digits) = settings
        .and_then(|settings| settings.split_once(';'))
        .unwrap_or_default();
    let steam = digits.trim() == "S";
    let digits = match steam {
        true => Some(STEAM_DIGITS),
        false => number(Some(digits), DEFAULT_DIGITS),
    };
    let mut totp = Totp::new(
        seed,
        TotpAlgorithm::Sha1,
        digits.ok_or(TotpError::InvalidDigits)?,
        number(Some(period), DEFAULT_PERIOD).ok_or(TotpError::InvalidPeriod)?,
    )?;
    totp.steam = steam;
    Ok(totp)
}

fn read_otp(otp: &str) -> Result<Totp, TotpError> {
    let otp = otp.trim();
    if is_otpauth(otp) {
        return Totp::from_uri(otp);
    }
    let settings: Vec<(String, Zeroizing<String>)> = url::form_urlencoded::parse(otp.as_bytes())
        .map(|(key, value)| (key.into_owned(), Zeroizing::new(value.into_owned())))
        .collect();
    let setting = |name: &str| {
        settings
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    match setting("key") {
        // The KeeOtp plugin's settings.
        Some(key) => Totp::new(
            key,
            setting("otpHashMode").map_or(TotpAlgorithm::Sha1, algorithm),
            number(setting("size"), DEFAULT_DIGITS).ok_or(TotpError::InvalidDigits)?,
            number(setting("step"), DEFAULT_PERIOD).ok_or(TotpError::InvalidPeriod)?,
        ),
        // A bare secret, which KeeWeb reads with the usual settings.
        None => Totp::parse(otp),
    }
}

fn read_keepass2(
    (encoding, secret): (&str, Zeroizing<String>),
    field: Fields,
) -> Result<Totp, TotpError> {
    let secret = match encoding {
        KEEPASS2_BASE32 => decode_base32(&secret)?,
        "TimeOtp-Secret" => Zeroizing::new(secret.as_bytes().to_vec()),
        "TimeOtp-Secret-Hex" => decode_hex(&secret)?,
        _ => Zeroizing::new(
            BASE64
                .decode(secret.trim())
                .map_err(|_| TotpError::InvalidSecret)?,
        ),
    };
    let setting = |name| field(name);
    let algorithm =
        setting(KEEPASS2_ALGORITHM).map_or(TotpAlgorithm::Sha1, |name| algorithm(&name));
    let digits = number(
        setting(KEEPASS2_LENGTH).as_deref().map(|s| s.as_str()),
        DEFAULT_DIGITS,
    )
    .ok_or(TotpError::InvalidDigits)?;
    let period = number(
        setting(KEEPASS2_PERIOD).as_deref().map(|s| s.as_str()),
        DEFAULT_PERIOD,
    )
    .ok_or(TotpError::InvalidPeriod)?;
    Totp::build(secret, algorithm, digits, period)
}

fn decode_hex(text: &str) -> Result<Zeroizing<Vec<u8>>, TotpError> {
    let digits: Zeroizing<Vec<u8>> =
        Zeroizing::new(text.bytes().filter(|b| !b.is_ascii_whitespace()).collect());
    if !digits.len().is_multiple_of(2) {
        return Err(TotpError::InvalidSecret);
    }
    let bytes = digits
        .chunks(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).map_err(|_| TotpError::InvalidSecret)?;
            u8::from_str_radix(pair, 16).map_err(|_| TotpError::InvalidSecret)
        })
        .collect::<Result<Vec<u8>, _>>()?;
    match bytes.is_empty() {
        true => Err(TotpError::MissingSecret),
        false => Ok(Zeroizing::new(bytes)),
    }
}

/// A field to write: its name, value, and whether the value is a secret to store protected.
pub(crate) struct Field {
    pub name: &'static str,
    pub value: Zeroizing<String>,
    pub protect: bool,
}

/// The fields that store `totp` in an Entry whose fields are now `field`: the formats the Entry
/// already uses, where they can hold the settings, and an `otpauth://` URI in `otp` when the
/// Entry uses that or nothing else can hold them.
pub(crate) fn write(totp: &Totp, field: Fields, title: &str, username: &str) -> Vec<Field> {
    let otp = field(OTP);
    let keeotp = otp.as_ref().is_some_and(|otp| {
        !is_otpauth(otp.trim())
            && url::form_urlencoded::parse(otp.trim().as_bytes()).any(|(key, _)| key == "key")
    });
    let legacy = field(LEGACY_SEED).is_some() || field(LEGACY_SETTINGS).is_some();
    let keepass2 = KEEPASS2_SECRETS.iter().any(|&name| field(name).is_some());

    let new = |name, value: String, protect| Field {
        name,
        value: Zeroizing::new(value),
        protect,
    };
    let secret = encode_base32(&totp.secret, false);
    let mut fields = Vec::new();
    // KeePassXC's legacy fields can't name a hash.
    if legacy && totp.algorithm == TotpAlgorithm::Sha1 {
        let digits = match totp.steam {
            true => "S".to_owned(),
            false => totp.digits.to_string(),
        };
        fields.push(new(LEGACY_SEED, secret.to_string(), true));
        fields.push(new(
            LEGACY_SETTINGS,
            format!("{};{digits}", totp.period),
            false,
        ));
    }
    // Neither KeePass2 nor KeeOtp has Steam codes.
    if keepass2 && !totp.steam {
        let algorithm = match totp.algorithm {
            TotpAlgorithm::Sha1 => "HMAC-SHA-1",
            TotpAlgorithm::Sha256 => "HMAC-SHA-256",
            TotpAlgorithm::Sha512 => "HMAC-SHA-512",
        };
        let padded = encode_base32(&totp.secret, true);
        fields.push(new(KEEPASS2_BASE32, padded.to_string(), true));
        fields.push(new(KEEPASS2_LENGTH, totp.digits.to_string(), false));
        fields.push(new(KEEPASS2_PERIOD, totp.period.to_string(), false));
        fields.push(new(KEEPASS2_ALGORITHM, algorithm.to_owned(), false));
    }
    if keeotp && !totp.steam {
        let mut settings = format!(
            "key={}&size={}&step={}",
            secret.as_str(),
            totp.digits,
            totp.period
        );
        if totp.algorithm != TotpAlgorithm::Sha1 {
            settings.push_str("&otpHashMode=");
            settings.push_str(algorithm_name(totp.algorithm));
        }
        fields.push(new(OTP, settings, true));
    } else if (otp.is_some() && !keeotp) || fields.is_empty() {
        let uri = totp.to_uri(title, username);
        fields.push(new(
            OTP,
            secrecy::ExposeSecret::expose_secret(&uri).to_owned(),
            true,
        ));
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_round_trips() {
        for length in 0..12 {
            let bytes: Vec<u8> = (0..length).map(|i: u8| i.wrapping_mul(37) + 1).collect();
            for pad in [false, true] {
                let text = encode_base32(&bytes, pad);
                if pad {
                    assert_eq!(text.len() % 8, 0, "{}", text.as_str());
                }
                if length > 0 {
                    assert_eq!(*decode_base32(&text).unwrap(), bytes, "{}", text.as_str());
                }
            }
        }
        assert_eq!(encode_base32(b"f", true).as_str(), "MY======");
        assert_eq!(encode_base32(b"foobar", false).as_str(), "MZXW6YTBOI");
    }

    #[test]
    fn reads_keepass2_secrets_in_any_encoding() {
        let expected = Totp::new(
            "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ",
            TotpAlgorithm::Sha1,
            6,
            30,
        );
        for (name, value) in [
            ("TimeOtp-Secret", "12345678901234567890"),
            (
                "TimeOtp-Secret-Hex",
                "3132333435363738393031323334353637383930",
            ),
            ("TimeOtp-Secret-Base64", "MTIzNDU2Nzg5MDEyMzQ1Njc4OTA="),
        ] {
            let field = |key: &str| (key == name).then(|| Zeroizing::new(value.to_owned()));
            assert_eq!(read(&field), Some(expected.clone()), "{name}");
        }
    }
}
