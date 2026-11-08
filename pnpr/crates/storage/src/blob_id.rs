//! The identity of bytes in the hosted store, and the key each incarnation
//! of them is stored under.
//!
//! A namespace stores bytes once, by the digest of its ecosystem: npm's
//! `sha512` integrity, and the `sha256` that images, crates, and Python
//! distributions are checked by. The bytes of one identity are stored under
//! `<identity>.<incarnation>`, where the metadata database counts
//! incarnations up and never reuses one (see [`crate::metadata::BlobRow`]).

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use pnpr_package_name::Ecosystem;
use ssri::{Algorithm, Integrity, IntegrityOpts};
use std::{fmt, io::Read, path::Path};

/// The digest algorithms an identity is spelled with, and their hex length.
const ALGORITHMS: [(&str, usize); 2] = [("sha256", 64), ("sha512", 128)];

/// What the bytes of a hosted archive are stored by: `<algorithm>-<hex>`,
/// with a lowercase hex digest, so one spelling names one set of bytes and
/// the identity is a safe path segment and object key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlobId(String);

impl BlobId {
    /// The identity `text` spells, or `None` when it is not one.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (algorithm, hex) = text.split_once('-')?;
        let (_, length) = ALGORITHMS
            .iter()
            .find(|(name, _)| *name == algorithm)?;
        let is_hex = hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        (hex.len() == *length && is_hex).then(|| Self(text.to_string()))
    }

    /// The identity `ecosystem` gives `bytes`.
    #[must_use]
    pub fn of_bytes(ecosystem: Ecosystem, bytes: &[u8]) -> Self {
        let mut hasher = IntegrityOpts::new().algorithm(algorithm_of(ecosystem));
        hasher.input(bytes);
        Self::from_integrity(&hasher.result(), ecosystem).expect("a digest of the algorithm")
    }

    /// The identity `ecosystem` gives the file at `path`, read in chunks.
    /// Blocking.
    pub fn of_file(ecosystem: Ecosystem, path: &Path) -> std::io::Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut hasher = IntegrityOpts::new().algorithm(algorithm_of(ecosystem));
        let mut buffer = vec![0; 64 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.input(&buffer[..read]);
        }
        Ok(Self::from_integrity(&hasher.result(), ecosystem).expect("a digest of the algorithm"))
    }

    /// The identity of the bytes `integrity` describes in `ecosystem`, when
    /// it carries the digest the ecosystem identifies bytes by.
    #[must_use]
    pub fn from_integrity(integrity: &Integrity, ecosystem: Ecosystem) -> Option<Self> {
        let algorithm = algorithm_of(ecosystem);
        let hash = integrity.hashes
            .iter()
            .find(|hash| hash.algorithm == algorithm)?;
        let digest = BASE64.decode(&hash.digest).ok()?;
        Self::parse(&format!("{algorithm}-{}", to_hex(&digest)))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The key incarnation `incarnation` of these bytes is stored under.
    #[must_use]
    pub fn key(&self, incarnation: u64) -> String {
        format!("{}.{incarnation}", self.0)
    }
}

impl fmt::Display for BlobId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The identity and incarnation a stored key names, or `None` for a name
/// pnpr does not store archives under.
#[must_use]
pub fn parse_blob_key(key: &str) -> Option<(BlobId, u64)> {
    let (id, incarnation) = key.rsplit_once('.')?;
    let valid = !incarnation.is_empty()
        && !incarnation.starts_with('0')
        && incarnation.bytes().all(|byte| byte.is_ascii_digit());
    Some((BlobId::parse(id)?, valid.then(|| incarnation.parse().ok())??))
}

/// The algorithm `ecosystem` identifies bytes by.
fn algorithm_of(ecosystem: Ecosystem) -> Algorithm {
    match ecosystem {
        Ecosystem::Npm => Algorithm::Sha512,
        Ecosystem::Cargo | Ecosystem::Pypi | Ecosystem::Oci => Algorithm::Sha256,
    }
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex.push(char::from(HEX[usize::from(byte >> 4)]));
        hex.push(char::from(HEX[usize::from(byte & 0xf)]));
    }
    hex
}

#[cfg(test)]
mod tests;
