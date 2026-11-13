//! TOTP: codes per RFC 6238, read from and written to the fields KeePassXC, KeePass2 and KeeWeb
//! keep them in, so the same Vault file gives the same codes in every client.

use keefast_core::{
    CompositeKey, EditError, Entry, EntryFields, Timestamp, Totp, TotpAlgorithm, TotpError, Vault,
};
use keepass::{Database, DatabaseKey};
use secrecy::{ExposeSecret, SecretString};

/// TOTP settings in every field format; see scripts/make-totp-fixture.sh.
const TOTP_KDBX: &[u8] = include_bytes!("../../../fixtures/totp.kdbx");
const PASSWORD: &str = "fixture-password";

/// RFC 6238's test secrets, Base32-encoded: "12345678901234567890" repeated to the length of
/// each hash.
const SHA1_SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
const SHA256_SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZA====";
const SHA512_SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ\
                             GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNA=";

fn open() -> Vault {
    Vault::open(TOTP_KDBX, &CompositeKey::password(PASSWORD)).expect("Vault file opens")
}

fn reopen(vault: &Vault) -> Vault {
    Vault::open(&vault.save().unwrap(), &CompositeKey::password(PASSWORD)).unwrap()
}

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_unix_seconds(seconds)
}

fn now() -> Timestamp {
    at(1_800_000_000)
}

fn entry<'a>(vault: &'a Vault, title: &str) -> &'a Entry {
    vault
        .root()
        .all_entries()
        .find(|entry| entry.title() == title)
        .unwrap_or_else(|| panic!("no Entry {title}"))
}

fn totp_of(vault: &Vault, title: &str) -> Totp {
    entry(vault, title)
        .totp()
        .unwrap_or_else(|| panic!("{title} has no TOTP"))
        .unwrap_or_else(|error| panic!("{title}'s TOTP is invalid: {error}"))
        .clone()
}

fn code(totp: &Totp, seconds: i64) -> String {
    totp.code(at(seconds)).code().to_owned()
}

/// The Entry's fields besides the standard ones, as an independent KDBX reader sees them:
/// name, value and whether it is protected, sorted by name.
fn independent_fields(vault: &Vault, title: &str) -> Vec<(String, String, bool)> {
    let bytes = vault.save().unwrap();
    let database = Database::parse(&bytes, DatabaseKey::new().with_password(PASSWORD)).unwrap();
    let entry = database
        .iter_all_entries()
        .find(|entry| entry.get_title() == Some(title))
        .unwrap_or_else(|| panic!("no Entry {title}"));
    let standard = ["Title", "UserName", "Password", "URL", "Notes"];
    let mut fields: Vec<_> = entry
        .fields
        .iter()
        .filter(|(key, _)| !standard.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.get().clone(), value.is_protected()))
        .collect();
    fields.sort();
    fields
}

#[test]
fn generates_the_rfc_6238_test_vectors() {
    let totp = |secret, algorithm| Totp::new(secret, algorithm, 8, 30).unwrap();
    let sha1 = totp(SHA1_SECRET, TotpAlgorithm::Sha1);
    let sha256 = totp(SHA256_SECRET, TotpAlgorithm::Sha256);
    let sha512 = totp(SHA512_SECRET, TotpAlgorithm::Sha512);
    // RFC 6238 appendix B.
    let vectors = [
        (59, "94287082", "46119246", "90693936"),
        (1_111_111_109, "07081804", "68084774", "25091201"),
        (1_111_111_111, "14050471", "67062674", "99943326"),
        (1_234_567_890, "89005924", "91819424", "93441116"),
        (2_000_000_000, "69279037", "90698825", "38618901"),
        (20_000_000_000, "65353130", "77737706", "47863826"),
    ];
    for (time, expected_sha1, expected_sha256, expected_sha512) in vectors {
        assert_eq!(code(&sha1, time), expected_sha1, "SHA-1 at {time}");
        assert_eq!(code(&sha256, time), expected_sha256, "SHA-256 at {time}");
        assert_eq!(code(&sha512, time), expected_sha512, "SHA-512 at {time}");
    }
}

#[test]
fn a_code_says_how_long_it_stays_valid() {
    let totp = Totp::new(SHA1_SECRET, TotpAlgorithm::Sha1, 6, 30).unwrap();
    let code = totp.code(at(1_111_111_109));
    assert_eq!(code.code(), "081804");
    assert_eq!(code.period(), 30);
    // 1111111109 is 29 seconds into its 30 second step.
    assert_eq!(code.remaining(), 1);
    assert_eq!(totp.code(at(1_111_111_110)).remaining(), 30);
}

#[test]
fn a_code_comes_with_the_one_after_it() {
    let totp = Totp::new(SHA1_SECRET, TotpAlgorithm::Sha1, 8, 30).unwrap();
    // RFC 6238's SHA-1 vectors at 1111111109 and 1111111111, a step apart.
    let now = totp.code(at(1_111_111_109));
    assert_eq!((now.code(), now.next()), ("07081804", "14050471"));
    // Whenever in its step it is asked, the next code is the one the next step starts with.
    for seconds in [0, 1, 29, 30, 59, 1_800_000_000] {
        let next_step = (seconds / 30 + 1) * 30;
        assert_eq!(
            totp.code(at(seconds)).next(),
            code(&totp, next_step),
            "at {seconds}"
        );
    }
}

#[test]
fn secrets_by_hand_may_be_lowercase_spaced_and_unpadded() {
    let messy = Totp::new(
        "gezd gnbv gy3t qojq gezd-gnbv-gy3t-qojq",
        TotpAlgorithm::Sha1,
        6,
        30,
    )
    .unwrap();
    let clean = Totp::new(SHA1_SECRET, TotpAlgorithm::Sha1, 6, 30).unwrap();
    assert_eq!(messy, clean);
    let unpadded = Totp::new(
        SHA256_SECRET.trim_end_matches('='),
        TotpAlgorithm::Sha256,
        8,
        30,
    );
    assert_eq!(code(&unpadded.unwrap(), 59), "46119246");
}

#[test]
fn settings_by_hand_are_checked() {
    let new = |secret, digits, period| Totp::new(secret, TotpAlgorithm::Sha1, digits, period);
    assert_eq!(new("", 6, 30), Err(TotpError::MissingSecret));
    assert_eq!(new("not base32!", 6, 30), Err(TotpError::InvalidSecret));
    assert_eq!(new(SHA1_SECRET, 0, 30), Err(TotpError::InvalidDigits));
    assert_eq!(new(SHA1_SECRET, 11, 30), Err(TotpError::InvalidDigits));
    assert_eq!(new(SHA1_SECRET, 6, 0), Err(TotpError::InvalidPeriod));
    assert!(new(SHA1_SECRET, 10, 86_400).is_ok());
}

#[test]
fn reads_an_otpauth_uri() {
    let totp = Totp::from_uri(&format!(
        "otpauth://totp/Example:alice@example.com?secret={SHA256_SECRET}&issuer=Example\
         &algorithm=SHA256&digits=8&period=60"
    ))
    .unwrap();
    assert_eq!(totp.algorithm(), TotpAlgorithm::Sha256);
    assert_eq!((totp.digits(), totp.period()), (8, 60));
    assert_eq!(code(&totp, 119), "46119246");

    let defaults = Totp::from_uri(&format!("otpauth://totp/alice?secret={SHA1_SECRET}")).unwrap();
    assert_eq!(defaults.algorithm(), TotpAlgorithm::Sha1);
    assert_eq!((defaults.digits(), defaults.period()), (6, 30));
}

#[test]
fn turns_down_uris_that_are_not_totp() {
    let secret = format!("secret={SHA1_SECRET}");
    assert_eq!(
        Totp::from_uri(&format!("otpauth://hotp/alice?{secret}&counter=1")),
        Err(TotpError::NotTotp)
    );
    assert_eq!(
        Totp::from_uri(&format!("https://example.com/?{secret}")),
        Err(TotpError::NotTotp)
    );
    assert_eq!(
        Totp::from_uri("otpauth://totp/alice?digits=6"),
        Err(TotpError::MissingSecret)
    );
    assert_eq!(
        Totp::from_uri(&format!("otpauth://totp/alice?{secret}&digits=six")),
        Err(TotpError::InvalidDigits)
    );
}

#[test]
fn setup_text_is_a_uri_with_its_own_settings_or_a_secret() {
    let setup =
        |text: &str, digits, period| Totp::from_setup(text, TotpAlgorithm::Sha256, digits, period);
    // A URI's own settings win over the form's, even ones that aren't numbers.
    let uri = format!(" otpauth://totp/alice?secret={SHA1_SECRET}&digits=8");
    let from_uri = setup(&uri, "seven", "").unwrap();
    assert_eq!(
        (from_uri.algorithm(), from_uri.digits(), from_uri.period()),
        (TotpAlgorithm::Sha1, 8, 30)
    );
    let secret = setup(&format!("  {SHA1_SECRET} "), " 7 ", "45").unwrap();
    assert_eq!(
        (secret.algorithm(), secret.digits(), secret.period()),
        (TotpAlgorithm::Sha256, 7, 45)
    );
    let defaults = setup(SHA1_SECRET, "", " ").unwrap();
    assert_eq!((defaults.digits(), defaults.period()), (6, 30));
    assert_eq!(
        setup(SHA1_SECRET, "seven", "30"),
        Err(TotpError::InvalidDigits)
    );
    assert_eq!(setup(SHA1_SECRET, "6", "-1"), Err(TotpError::InvalidPeriod));
    assert_eq!(setup("  ", "6", "30"), Err(TotpError::MissingSecret));
}

#[test]
fn reads_every_clients_field_format() {
    let vault = open();
    let sha1 = Totp::new(SHA1_SECRET, TotpAlgorithm::Sha1, 6, 30).unwrap();

    assert_eq!(totp_of(&vault, "OTP URI"), sha1);
    assert_eq!(code(&totp_of(&vault, "OTP URI"), 59), "287082");

    let sha256 = totp_of(&vault, "OTP URI SHA-256");
    assert_eq!(
        (sha256.algorithm(), sha256.digits(), sha256.period()),
        (TotpAlgorithm::Sha256, 8, 60)
    );
    assert_eq!(code(&sha256, 119), "46119246");

    // KeePassXC's older fields, TOTP Seed and TOTP Settings.
    assert_eq!(code(&totp_of(&vault, "Legacy"), 59), "94287082");
    // The KeeOtp plugin's settings in the otp field.
    assert_eq!(code(&totp_of(&vault, "KeeOtp"), 59), "94287082");
    // A bare secret in the otp field, as KeeWeb reads it.
    assert_eq!(totp_of(&vault, "Bare secret"), sha1);
    // KeePass2's TimeOtp fields.
    let keepass2 = totp_of(&vault, "KeePass2");
    assert_eq!(keepass2.algorithm(), TotpAlgorithm::Sha512);
    assert_eq!(code(&keepass2, 59), "90693936");

    assert!(entry(&vault, "No TOTP").totp().is_none());
    assert_eq!(
        entry(&vault, "Broken").totp().map(|totp| totp.err()),
        Some(Some(TotpError::InvalidSecret))
    );
}

#[test]
fn generates_steam_codes_as_keepassxc_does() {
    let steam = totp_of(&open(), "Steam");
    assert!(steam.is_steam());
    assert_eq!(steam.digits(), 5);
    // Computed as KeePassXC does: the code's characters from Steam's alphabet, least
    // significant first.
    assert_eq!(code(&steam, 59), "PV9M4");
    assert_eq!(code(&steam, 1_791_412_350), "GC7J8");
    let from_uri = format!("otpauth://totp/Steam:alice?secret={SHA1_SECRET}&encoder=steam");
    assert_eq!(Totp::from_uri(&from_uri).unwrap(), steam);
}

#[test]
fn a_new_totp_is_written_as_an_otpauth_uri_like_keepassxc() {
    let mut vault = open();
    let id = entry(&vault, "No TOTP").id();
    let totp = Totp::new(SHA256_SECRET, TotpAlgorithm::Sha256, 8, 60).unwrap();
    vault.set_totp(id, Some(&totp), now()).unwrap();

    let vault = reopen(&vault);
    assert_eq!(totp_of(&vault, "No TOTP"), totp);
    assert_eq!(
        independent_fields(&vault, "No TOTP"),
        [(
            "otp".to_owned(),
            "otpauth://totp/No%20TOTP:alice?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZA\
             &period=60&digits=8&issuer=No%20TOTP&algorithm=SHA256"
                .to_owned(),
            true
        )]
    );
}

#[test]
fn a_uri_keeps_its_own_issuer_and_account() {
    let mut vault = open();
    let id = entry(&vault, "No TOTP").id();
    let totp = Totp::from_uri(&format!(
        "otpauth://totp/ACME%20Co:john.doe@email.com?secret={SHA1_SECRET}&issuer=ACME%20Co"
    ))
    .unwrap();
    vault.set_totp(id, Some(&totp), now()).unwrap();
    let fields = independent_fields(&vault, "No TOTP");
    assert_eq!(
        fields[0].1,
        format!(
            "otpauth://totp/ACME%20Co:john.doe%40email.com?secret={SHA1_SECRET}&period=30\
             &digits=6&issuer=ACME%20Co"
        )
    );
}

#[test]
fn steam_is_written_with_keepassxcs_encoder() {
    let mut vault = open();
    let id = entry(&vault, "No TOTP").id();
    let steam = totp_of(&vault, "Steam");
    vault.set_totp(id, Some(&steam), now()).unwrap();
    assert!(
        independent_fields(&vault, "No TOTP")[0]
            .1
            .ends_with("&encoder=steam")
    );
    assert_eq!(totp_of(&reopen(&vault), "No TOTP"), steam);
}

#[test]
fn changing_a_totp_keeps_the_entrys_format() {
    let mut vault = open();
    let changed = Totp::new(SHA1_SECRET, TotpAlgorithm::Sha1, 7, 45).unwrap();
    for title in ["Legacy", "KeeOtp", "KeePass2"] {
        let id = entry(&vault, title).id();
        vault.set_totp(id, Some(&changed), now()).unwrap();
    }
    let vault = reopen(&vault);
    for title in ["Legacy", "KeeOtp", "KeePass2"] {
        assert_eq!(totp_of(&vault, title), changed, "{title}");
    }
    let field = |key: &str, value: &str, protected| (key.to_owned(), value.to_owned(), protected);
    assert_eq!(
        independent_fields(&vault, "Legacy"),
        [
            field("TOTP Seed", SHA1_SECRET, true),
            field("TOTP Settings", "45;7", false)
        ]
    );
    assert_eq!(
        independent_fields(&vault, "KeeOtp"),
        [field(
            "otp",
            &format!("key={SHA1_SECRET}&size=7&step=45"),
            true
        )]
    );
    assert_eq!(
        independent_fields(&vault, "KeePass2"),
        [
            field("TimeOtp-Algorithm", "HMAC-SHA-1", false),
            field("TimeOtp-Length", "7", false),
            field("TimeOtp-Period", "45", false),
            field("TimeOtp-Secret-Base32", SHA1_SECRET, true),
        ]
    );
}

#[test]
fn settings_an_old_format_cant_hold_go_to_an_otpauth_uri() {
    let mut vault = open();
    // KeePassXC's legacy fields have no algorithm and only 30 second Steam codes, and
    // KeePass2's and KeeOtp's no Steam at all.
    let sha256 = Totp::new(SHA256_SECRET, TotpAlgorithm::Sha256, 6, 30).unwrap();
    let steam = totp_of(&vault, "Steam");
    let slow_steam = Totp::from_uri(&format!(
        "otpauth://totp/Steam:alice?secret={SHA1_SECRET}&encoder=steam&period=60"
    ))
    .unwrap();
    let legacy = entry(&vault, "Legacy").id();
    let keepass2 = entry(&vault, "KeePass2").id();
    let keeotp = entry(&vault, "KeeOtp").id();
    vault.set_totp(legacy, Some(&sha256), now()).unwrap();
    vault.set_totp(keepass2, Some(&steam), now()).unwrap();
    vault.set_totp(keeotp, Some(&slow_steam), now()).unwrap();

    let vault = reopen(&vault);
    assert_eq!(totp_of(&vault, "Legacy"), sha256);
    assert_eq!(totp_of(&vault, "KeePass2"), steam);
    assert_eq!(totp_of(&vault, "KeeOtp"), slow_steam);
    for title in ["Legacy", "KeePass2", "KeeOtp"] {
        let fields = independent_fields(&vault, title);
        assert_eq!(fields.len(), 1, "{title}: {fields:?}");
        assert_eq!(fields[0].0, "otp");
    }
}

#[test]
fn removing_a_totp_clears_every_format() {
    let mut vault = open();
    for title in ["OTP URI", "Legacy", "KeePass2", "Broken"] {
        let id = entry(&vault, title).id();
        vault.set_totp(id, None, now()).unwrap();
    }
    let vault = reopen(&vault);
    for title in ["OTP URI", "Legacy", "KeePass2", "Broken"] {
        assert!(entry(&vault, title).totp().is_none(), "{title}");
        assert_eq!(independent_fields(&vault, title), [], "{title}");
    }
}

#[test]
fn a_totp_change_is_an_edit_with_history() {
    let mut vault = open();
    let id = entry(&vault, "OTP URI").id();
    let before = entry(&vault, "OTP URI").clone();
    let changed = Totp::new(SHA1_SECRET, TotpAlgorithm::Sha1, 8, 30).unwrap();
    vault.set_totp(id, Some(&changed), now()).unwrap();

    let vault = reopen(&vault);
    let after = entry(&vault, "OTP URI");
    assert_eq!(after.last_modified(), Some(now()));
    assert_eq!(after.history().len(), before.history().len() + 1);
    let previous = after.history().last().unwrap();
    assert_eq!(previous.totp().unwrap().unwrap().digits(), 6);
}

#[test]
fn setting_the_same_totp_changes_nothing() {
    let mut vault = open();
    let id = entry(&vault, "OTP URI").id();
    let same = totp_of(&vault, "OTP URI");
    let before = vault.clone();
    vault.set_totp(id, Some(&same), now()).unwrap();
    vault
        .set_totp(entry(&vault, "No TOTP").id(), None, now())
        .unwrap();
    assert!(vault == before);
}

#[test]
fn editing_other_fields_keeps_the_totp() {
    let mut vault = open();
    let id = entry(&vault, "KeePass2").id();
    let fields = EntryFields {
        title: "Renamed".into(),
        password: SecretString::from("new password"),
        ..EntryFields::from(entry(&vault, "KeePass2"))
    };
    vault.update_entry(id, &fields, now()).unwrap();
    let vault = reopen(&vault);
    assert_eq!(code(&totp_of(&vault, "Renamed"), 59), "90693936");
    assert_eq!(
        entry(&vault, "Renamed").password().expose_secret(),
        "new password"
    );
}

#[test]
fn a_totp_can_be_set_only_on_an_entry_in_the_vault() {
    let mut vault = open();
    let elsewhere = {
        let mut other = open();
        let root = other.root().id();
        other
            .create_entry(root, &EntryFields::default(), now())
            .unwrap()
    };
    assert_eq!(
        vault.set_totp(elsewhere, None, now()),
        Err(EditError::EntryNotFound)
    );
}

#[test]
fn a_totp_shows_its_secret_and_uri_for_editing() {
    let totp = totp_of(&open(), "Bare secret");
    assert_eq!(totp.secret().expose_secret(), SHA1_SECRET);
    assert_eq!(
        totp.to_uri("GitHub", "").expose_secret(),
        format!("otpauth://totp/GitHub:none?secret={SHA1_SECRET}&period=30&digits=6&issuer=GitHub")
    );
    assert!(!format!("{totp:?}").contains(SHA1_SECRET));
    // KeeWeb gives Steam codes only for TOTPs issued by Steam.
    let steam = Totp::from_uri(&format!(
        "otpauth://totp/Valve:alice?secret={SHA1_SECRET}&encoder=steam"
    ))
    .unwrap();
    assert!(
        steam
            .to_uri("Games", "alice")
            .expose_secret()
            .starts_with("otpauth://totp/Steam:alice?")
    );
    assert!(
        steam
            .to_uri("Games", "alice")
            .expose_secret()
            .contains("&issuer=Steam&")
    );
}
