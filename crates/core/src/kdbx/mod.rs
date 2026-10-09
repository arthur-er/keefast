//! Reading and writing Vault files (KDBX).
//!
//! keefast reads KDBX 3.1 and 4.x and writes KDBX 4 (ADR 0002). A Vault file is held as its
//! whole XML document plus the settings and attachment pool around it, always in KDBX 4 form,
//! so a save writes back everything that was read, including what keefast doesn't model.

mod crypto;
mod header;
mod upgrade;

use std::io::{Read, Write};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::xml::{self, Element, Node, Secret};
use crate::{Argon2, Cipher, Encryption, Kdf, OpenError, SaveError};
use crypto::{InnerStream, OuterCipher};
use header::{Field, VariantDictionary, Version, field, inner_field};

pub(crate) use crypto::{Key, random};

/// The contents of a Vault file.
#[derive(Debug, Clone)]
pub(crate) struct VaultFile {
    pub(crate) settings: Settings,
    /// The attachment pool that `<Binary><Value Ref="N"/>` refers to, by index.
    pub(crate) binaries: Vec<Binary>,
    /// The `KeePassFile` document, with protected values decrypted.
    pub(crate) document: Element,
}

/// How the Vault file is encrypted, plus header data keefast passes through untouched.
#[derive(Debug, Clone)]
pub(crate) struct Settings {
    /// Written as KDBX 4.`minor_version`.
    minor_version: u16,
    cipher: OuterCipher,
    cipher_uuid: Uuid,
    gzip: bool,
    /// KDF parameters. The seed is replaced on every save; everything else is kept.
    kdf: VariantDictionary,
    public_custom_data: Option<Vec<u8>>,
    other_fields: Vec<Field>,
    other_inner_fields: Vec<Field>,
}

impl Settings {
    /// A new KDBX 4.0 Vault file's: gzip-compressed, with no settings yet beyond those. Set the
    /// cipher and KDF with [`Self::set_encryption`].
    pub(crate) fn new_kdbx4() -> Self {
        Self {
            minor_version: 0,
            cipher: OuterCipher::Aes256,
            cipher_uuid: crypto::AES256,
            gzip: true,
            kdf: VariantDictionary::new(),
            public_custom_data: None,
            other_fields: Vec::new(),
            other_inner_fields: Vec::new(),
        }
    }

    pub(crate) fn encryption(&self) -> Encryption {
        let cipher = self.cipher.into();
        let kdf = &self.kdf;
        let argon2 = || Argon2 {
            memory: kdf.u64("M").unwrap_or_default(),
            iterations: kdf.u64("I").unwrap_or_default(),
            parallelism: kdf.u32("P").unwrap_or_default(),
        };
        let kdf = match kdf_uuid(kdf) {
            Some(crypto::ARGON2ID) => Kdf::Argon2id(argon2()),
            Some(crypto::ARGON2D) => Kdf::Argon2d(argon2()),
            _ => Kdf::AesKdf {
                rounds: kdf.u64("R").unwrap_or_default(),
            },
        };
        Encryption { cipher, kdf }
    }

    /// Sets the cipher and KDF. A KDF that stays the same keeps the parameters keefast doesn't
    /// set, such as Argon2's version; a new one starts afresh. The seed is new on every save.
    pub(crate) fn set_encryption(&mut self, encryption: &Encryption) {
        let cipher = OuterCipher::from(encryption.cipher);
        if cipher != self.cipher {
            self.cipher = cipher;
            self.cipher_uuid = cipher.uuid();
        }
        let uuid = match encryption.kdf {
            Kdf::Argon2id(_) => crypto::ARGON2ID,
            Kdf::Argon2d(_) => crypto::ARGON2D,
            Kdf::AesKdf { .. } => crypto::AES_KDF,
        };
        let same_kdf = match kdf_uuid(&self.kdf) {
            Some(crypto::AES_KDF_KDBX4) => uuid == crypto::AES_KDF,
            current => current == Some(uuid),
        };
        if !same_kdf {
            self.kdf = VariantDictionary::new();
            self.kdf.set_bytes("$UUID", uuid.as_bytes());
            self.kdf.set_bytes("S", &crypto::random::<32>());
        }
        match encryption.kdf {
            Kdf::Argon2id(argon2) | Kdf::Argon2d(argon2) => {
                if !same_kdf {
                    self.kdf.set_u32("V", ARGON2_VERSION);
                }
                self.kdf.set_u64("M", argon2.memory);
                self.kdf.set_u64("I", argon2.iterations);
                self.kdf.set_u32("P", argon2.parallelism);
            }
            Kdf::AesKdf { rounds } => self.kdf.set_u64("R", rounds),
        }
    }
}

impl From<OuterCipher> for Cipher {
    fn from(cipher: OuterCipher) -> Self {
        match cipher {
            OuterCipher::Aes256 => Cipher::Aes256,
            OuterCipher::ChaCha20 => Cipher::ChaCha20,
            OuterCipher::Twofish => Cipher::Twofish,
        }
    }
}

impl From<Cipher> for OuterCipher {
    fn from(cipher: Cipher) -> Self {
        match cipher {
            Cipher::Aes256 => OuterCipher::Aes256,
            Cipher::ChaCha20 => OuterCipher::ChaCha20,
            Cipher::Twofish => OuterCipher::Twofish,
        }
    }
}

/// Argon2 1.3, the version every client writes.
const ARGON2_VERSION: u32 = 0x13;

fn kdf_uuid(kdf: &VariantDictionary) -> Option<Uuid> {
    kdf.bytes("$UUID")
        .and_then(|bytes| Uuid::from_slice(bytes).ok())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Binary {
    /// Whether clients should keep the attachment protected in memory.
    pub(crate) protected: bool,
    pub(crate) data: Secret,
}

/// Decrypts a Vault file. `composite` is the hash of the composite key.
pub(crate) fn read(data: &[u8], composite: &[u8; 32]) -> Result<VaultFile, OpenError> {
    match header::version(data)? {
        Version::Kdbx3 { .. } => read_kdbx3(data, composite),
        Version::Kdbx4 { minor } => read_kdbx4(data, composite, minor),
    }
}

fn truncated() -> OpenError {
    OpenError::Corrupt("the Vault file is truncated".into())
}

fn missing(name: &str) -> OpenError {
    OpenError::Corrupt(format!("the header has no {name}"))
}

fn le_u32(bytes: &[u8], name: &str) -> Result<u32, OpenError> {
    Ok(u32::from_le_bytes(
        bytes.try_into().map_err(|_| missing(name))?,
    ))
}

fn le_u64(bytes: &[u8], name: &str) -> Result<u64, OpenError> {
    Ok(u64::from_le_bytes(
        bytes.try_into().map_err(|_| missing(name))?,
    ))
}

fn read_kdbx4(data: &[u8], composite: &[u8; 32], minor: u16) -> Result<VaultFile, OpenError> {
    let (fields, header_end) = header::read_fields(data, 12, true)?;
    let header_bytes = &data[..header_end];
    let header_hash = data
        .get(header_end..header_end + 32)
        .ok_or_else(truncated)?;
    let header_hmac = data
        .get(header_end + 32..header_end + 64)
        .ok_or_else(truncated)?;
    if header_hash != crypto::sha256(&[header_bytes]) {
        return Err(OpenError::Corrupt(
            "the header fails its integrity check".into(),
        ));
    }

    let mut cipher_uuid = None;
    let mut gzip = None;
    let mut master_seed = None;
    let mut iv = None;
    let mut kdf = None;
    let mut public_custom_data = None;
    let mut other_fields = Vec::new();
    for (id, value) in fields {
        match id {
            field::CIPHER => {
                cipher_uuid = Some(Uuid::from_slice(&value).map_err(|_| missing("cipher"))?)
            }
            field::COMPRESSION => gzip = Some(compression(le_u32(&value, "compression")?)?),
            field::MASTER_SEED => master_seed = Some(value),
            field::ENCRYPTION_IV => iv = Some(value),
            field::KDF_PARAMETERS => kdf = Some(VariantDictionary::parse(&value)?),
            field::PUBLIC_CUSTOM_DATA => public_custom_data = Some(value),
            _ => other_fields.push((id, value)),
        }
    }
    let cipher_uuid = cipher_uuid.ok_or_else(|| missing("cipher"))?;
    let cipher = OuterCipher::from_uuid(cipher_uuid)?;
    let master_seed = master_seed.ok_or_else(|| missing("master seed"))?;
    let iv = iv.ok_or_else(|| missing("encryption IV"))?;
    let kdf = kdf.ok_or_else(|| missing("KDF parameters"))?;

    let transformed = crypto::transform_key(&kdf, composite)?;
    let hmac_key = crypto::hmac_key(&master_seed, &transformed);
    if header_hmac != crypto::header_hmac(header_bytes, &hmac_key) {
        return Err(OpenError::WrongKey);
    }

    let encrypted = crypto::read_hmac_blocks(&data[header_end + 64..], &hmac_key)?;
    let key = crypto::cipher_key(&master_seed, &transformed);
    let mut payload = cipher.decrypt_raw(&key, &iv, &encrypted)?;
    if cipher.is_block_cipher() {
        payload = crypto::pkcs7_unpad(payload)?;
    }
    let gzip = gzip.ok_or_else(|| missing("compression"))?;
    if gzip {
        payload = gunzip(&payload)?;
    }

    let (inner_fields, inner_end) = header::read_fields(&payload, 0, true)?;
    let mut stream_id = None;
    let mut stream_key = None;
    let mut binaries = Vec::new();
    let mut other_inner_fields = Vec::new();
    for (id, value) in inner_fields {
        match id {
            inner_field::RANDOM_STREAM_ID => {
                stream_id = Some(le_u32(&value, "inner random stream")?)
            }
            inner_field::RANDOM_STREAM_KEY => stream_key = Some(Zeroizing::new(value)),
            inner_field::BINARY => {
                let mut data = value;
                if data.is_empty() {
                    return Err(missing("attachment flags"));
                }
                // Shifts in place, so the attachment is never copied unwiped.
                let flags = data.remove(0);
                binaries.push(Binary {
                    protected: flags & 1 != 0,
                    data: Secret::new(data),
                });
            }
            _ => other_inner_fields.push((id, value)),
        }
    }
    let stream_id = stream_id.ok_or_else(|| missing("inner random stream"))?;
    let stream_key = stream_key.ok_or_else(|| missing("inner random stream key"))?;
    let mut stream = InnerStream::new(stream_id, &stream_key)?;
    let document = parse_document(&payload[inner_end..], &mut stream)?;

    Ok(VaultFile {
        settings: Settings {
            minor_version: minor,
            cipher,
            cipher_uuid,
            gzip,
            kdf,
            public_custom_data,
            other_fields,
            other_inner_fields,
        },
        binaries,
        document,
    })
}

fn read_kdbx3(data: &[u8], composite: &[u8; 32]) -> Result<VaultFile, OpenError> {
    let (fields, header_end) = header::read_fields(data, 12, false)?;

    let mut cipher_uuid = None;
    let mut gzip = None;
    let mut master_seed = None;
    let mut transform_seed = None;
    let mut rounds = None;
    let mut iv = None;
    let mut stream_key = None;
    let mut start_bytes = None;
    let mut stream_id = None;
    let mut other_fields = Vec::new();
    for (id, value) in fields {
        match id {
            field::CIPHER => {
                cipher_uuid = Some(Uuid::from_slice(&value).map_err(|_| missing("cipher"))?)
            }
            field::COMPRESSION => gzip = Some(compression(le_u32(&value, "compression")?)?),
            field::MASTER_SEED => master_seed = Some(value),
            field::TRANSFORM_SEED => transform_seed = Some(value),
            field::TRANSFORM_ROUNDS => rounds = Some(le_u64(&value, "transform rounds")?),
            field::ENCRYPTION_IV => iv = Some(value),
            field::PROTECTED_STREAM_KEY => stream_key = Some(Zeroizing::new(value)),
            field::STREAM_START_BYTES => start_bytes = Some(value),
            field::INNER_RANDOM_STREAM => stream_id = Some(le_u32(&value, "inner random stream")?),
            _ => other_fields.push((id, value)),
        }
    }
    let cipher_uuid = cipher_uuid.ok_or_else(|| missing("cipher"))?;
    let cipher = OuterCipher::from_uuid(cipher_uuid)?;
    let master_seed = master_seed.ok_or_else(|| missing("master seed"))?;
    let iv = iv.ok_or_else(|| missing("encryption IV"))?;
    let start_bytes = start_bytes.ok_or_else(|| missing("stream start bytes"))?;

    // KDBX 3.1 always uses AES-KDF; KDBX 4 describes it with KDF parameters.
    let mut kdf = VariantDictionary::new();
    kdf.set_bytes("$UUID", crypto::AES_KDF.as_bytes());
    kdf.set_u64("R", rounds.ok_or_else(|| missing("transform rounds"))?);
    kdf.set_bytes(
        "S",
        &transform_seed.ok_or_else(|| missing("transform seed"))?,
    );

    let transformed = crypto::transform_key(&kdf, composite)?;
    let key = crypto::cipher_key(&master_seed, &transformed);
    let mut payload = cipher.decrypt_raw(&key, &iv, &data[header_end..])?;
    // The first bytes of the payload repeat a header field: the only key check KDBX 3.1 has.
    if payload.get(..start_bytes.len()) != Some(start_bytes.as_slice()) {
        return Err(OpenError::WrongKey);
    }
    if cipher.is_block_cipher() {
        payload = crypto::pkcs7_unpad(payload)?;
    }
    let mut payload = Zeroizing::new(crypto::read_hashed_blocks(&payload[start_bytes.len()..])?);
    let gzip = gzip.ok_or_else(|| missing("compression"))?;
    if gzip {
        payload = gunzip(&payload)?;
    }

    let stream_id = stream_id.ok_or_else(|| missing("inner random stream"))?;
    let stream_key = stream_key.ok_or_else(|| missing("inner random stream key"))?;
    let mut stream = InnerStream::new(stream_id, &stream_key)?;
    let mut document = parse_document(&payload, &mut stream)?;
    // KDBX 3.1 authenticates its header only through this hash in the XML, when present.
    if let Some(hash) = document
        .child("Meta")
        .and_then(|meta| meta.child("HeaderHash"))
    {
        let expected = crypto::sha256(&[&data[..header_end]]);
        if BASE64.decode(hash.text().trim()).ok().as_deref() != Some(&expected[..]) {
            return Err(OpenError::Corrupt(
                "the header fails its integrity check".into(),
            ));
        }
    }
    let binaries = upgrade::upgrade(&mut document)?;

    Ok(VaultFile {
        settings: Settings {
            minor_version: 0,
            cipher,
            cipher_uuid,
            gzip,
            kdf,
            public_custom_data: None,
            other_fields,
            other_inner_fields: Vec::new(),
        },
        binaries,
        document,
    })
}

fn compression(id: u32) -> Result<bool, OpenError> {
    match id {
        0 => Ok(false),
        1 => Ok(true),
        other => Err(OpenError::Unsupported(format!("compression {other}"))),
    }
}

fn gunzip(data: &[u8]) -> Result<Zeroizing<Vec<u8>>, OpenError> {
    let mut out = Zeroizing::new(Vec::new());
    flate2::read::GzDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|error| OpenError::Corrupt(format!("the payload does not decompress: {error}")))?;
    Ok(out)
}

fn parse_document(xml: &[u8], stream: &mut InnerStream) -> Result<Element, OpenError> {
    let corrupt =
        |reason: String| OpenError::Corrupt(format!("the Vault's XML is invalid: {reason}"));
    let xml = std::str::from_utf8(xml).map_err(|error| corrupt(error.to_string()))?;
    let mut document = xml::parse(xml).map_err(|error| corrupt(error.to_string()))?;
    if document.name != "KeePassFile" {
        return Err(corrupt(format!(
            "unexpected root element {}",
            document.name
        )));
    }
    unprotect(&mut document, stream)?;
    Ok(document)
}

/// Decrypts protected values in document order, the order the stream was applied in.
fn unprotect(element: &mut Element, stream: &mut InnerStream) -> Result<(), OpenError> {
    if element.is_protected() && element.elements().next().is_none() {
        let mut bytes = BASE64.decode(element.text().trim()).map_err(|error| {
            OpenError::Corrupt(format!("a protected value is not Base64: {error}"))
        })?;
        stream.apply(&mut bytes);
        element.children = vec![Node::Protected(Secret::new(bytes))];
        return Ok(());
    }
    for child in element.elements_mut() {
        unprotect(child, stream)?;
    }
    Ok(())
}

/// Encrypts a Vault file as KDBX 4 with fresh seeds, keeping every other setting.
pub(crate) fn write(file: &VaultFile, composite: &[u8; 32]) -> Result<Vec<u8>, SaveError> {
    let settings = &file.settings;
    let master_seed: [u8; 32] = crypto::random();
    let iv = &crypto::random::<16>()[..settings.cipher.iv_len()];
    let mut kdf = settings.kdf.clone();
    kdf.set_bytes("S", &crypto::random::<32>());
    let transformed = crypto::transform_key(&kdf, composite)
        .map_err(|error| SaveError::Settings(error.to_string()))?;

    let mut out = Vec::new();
    header::write_version(&mut out, settings.minor_version);
    header::write_field(&mut out, field::CIPHER, settings.cipher_uuid.as_bytes());
    header::write_field(
        &mut out,
        field::COMPRESSION,
        &u32::from(settings.gzip).to_le_bytes(),
    );
    header::write_field(&mut out, field::MASTER_SEED, &master_seed);
    header::write_field(&mut out, field::ENCRYPTION_IV, iv);
    header::write_field(&mut out, field::KDF_PARAMETERS, &kdf.to_bytes());
    if let Some(data) = &settings.public_custom_data {
        header::write_field(&mut out, field::PUBLIC_CUSTOM_DATA, data);
    }
    for (id, value) in &settings.other_fields {
        header::write_field(&mut out, *id, value);
    }
    header::write_field(&mut out, field::END, b"\r\n\r\n");

    let hmac_key = crypto::hmac_key(&master_seed, &transformed);
    let header_hash = crypto::sha256(&[&out]);
    let header_hmac = crypto::header_hmac(&out, &hmac_key);
    out.extend_from_slice(&header_hash);
    out.extend_from_slice(&header_hmac);

    let stream_key: Zeroizing<[u8; 64]> = Zeroizing::new(crypto::random());
    let mut payload = Zeroizing::new(Vec::new());
    header::write_field(
        &mut payload,
        inner_field::RANDOM_STREAM_ID,
        &crypto::INNER_STREAM_CHACHA20.to_le_bytes(),
    );
    header::write_field(
        &mut payload,
        inner_field::RANDOM_STREAM_KEY,
        &stream_key[..],
    );
    for binary in &file.binaries {
        let mut value = Zeroizing::new(Vec::with_capacity(binary.data.bytes().len() + 1));
        value.push(u8::from(binary.protected));
        value.extend_from_slice(binary.data.bytes());
        header::write_field(&mut payload, inner_field::BINARY, &value);
    }
    for (id, value) in &settings.other_inner_fields {
        header::write_field(&mut payload, *id, value);
    }
    header::write_field(&mut payload, inner_field::END, &[]);

    let mut stream = InnerStream::new(crypto::INNER_STREAM_CHACHA20, &stream_key[..])
        .expect("ChaCha20 is a supported inner random stream");
    let xml = Zeroizing::new(xml::write(&file.document, &mut |plain| {
        let mut bytes = plain.to_vec();
        stream.apply(&mut bytes);
        BASE64.encode(bytes)
    }));
    payload.extend_from_slice(xml.as_bytes());

    if settings.gzip {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder
            .write_all(&payload)
            .expect("writing to memory cannot fail");
        payload = Zeroizing::new(encoder.finish().expect("writing to memory cannot fail"));
    }
    let key = crypto::cipher_key(&master_seed, &transformed);
    let encrypted = settings.cipher.encrypt(&key, iv, &payload);
    out.extend_from_slice(&crypto::write_hmac_blocks(&encrypted, &hmac_key));
    Ok(out)
}
