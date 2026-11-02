//! What identifies a record: its package or OCI tag, its revision, and its
//! profile, within the registry authority that logged it.

pub use log_entry::*;

mod log_entry;

use crate::{digest::lowercase_hex, profile::Profile, purl::Purl};
use derive_more::{Display, Error};
use sha2::{Digest as _, Sha256};
use std::{fmt, str::FromStr};

/// What a record describes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RecordIdentity {
    /// A released artifact, by its Package URL: an npm version, a crate
    /// version, or a Python distribution file.
    Package(Purl),
    /// An OCI tag, whose image digest changes from one revision to the next.
    OciTag {
        /// The repository, such as `acme/app`.
        repository: String,
        /// The tag, such as `1.0`.
        tag: String,
    },
}

/// The identity of one record: what it describes, its registry revision if
/// the ecosystem has revisions, and its profile. Two records with the same
/// key and different digests are a forged or rewritten record.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordKey {
    /// What the record describes.
    pub identity: RecordIdentity,
    /// The registry revision, absent for Cargo and Python records.
    pub revision: Option<u64>,
    /// The profile the record was derived under.
    pub profile: Profile,
}

impl RecordKey {
    /// The name a statement's subject gives the record: its PURL, or
    /// `<repository>:<tag>+r<revision>` for an OCI tag record.
    #[must_use]
    pub fn subject_name(&self) -> String {
        match &self.identity {
            RecordIdentity::Package(purl) => purl.to_string(),
            RecordIdentity::OciTag { repository, tag } => {
                format!("{repository}:{tag}+r{}", self.revision.unwrap_or_default())
            }
        }
    }
}

/// A registry's authority: its log name and the SHA-256 of the first version
/// of its TUF root, written `<log name>+sha256:<hex>`.
///
/// The authority stays the same when the registry moves to another host or
/// rotates a key, so a lockfile or cache keyed by it stays valid. Two
/// registries never share one, even when they serve identical bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Authority {
    log_name: String,
    first_root_digest: [u8; 32],
}

/// Why an authority string was rejected.
#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
#[display("not a <log name>+sha256:<hex> authority: {text}")]
pub struct AuthorityError {
    #[error(not(source))]
    text: String,
}

impl Authority {
    /// The authority of the log named `log_name` whose TUF repository's first
    /// root version has `first_root_digest` as its SHA-256.
    #[must_use]
    pub fn new(log_name: &str, first_root_digest: [u8; 32]) -> Self {
        Authority { log_name: log_name.to_owned(), first_root_digest }
    }

    /// The authority of the log named `log_name` whose TUF repository's first
    /// root version is `first_root`.
    #[must_use]
    pub fn of_first_root(log_name: &str, first_root: &[u8]) -> Self {
        Authority::new(log_name, Sha256::digest(first_root).into())
    }

    /// The log name, which is its checkpoints' origin.
    #[must_use]
    pub fn log_name(&self) -> &str {
        &self.log_name
    }

    /// The SHA-256 of the first TUF root version, as a workspace pins it.
    #[must_use]
    pub const fn first_root_digest(&self) -> &[u8; 32] {
        &self.first_root_digest
    }
}

impl fmt::Display for Authority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}+sha256:", self.log_name)?;
        self.first_root_digest
            .iter()
            .try_for_each(|byte| write!(formatter, "{byte:02x}"))
    }
}

impl FromStr for Authority {
    type Err = AuthorityError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let error = || AuthorityError { text: text.to_owned() };
        let (log_name, digest) = text.rsplit_once("+sha256:").ok_or_else(error)?;
        let is_valid_name = !log_name.is_empty() && !log_name.contains(char::is_whitespace);
        if !is_valid_name || log_name.contains('+') {
            return Err(error());
        }
        let first_root_digest = parse_lowercase_hex(digest).ok_or_else(error)?;
        Ok(Authority::new(log_name, first_root_digest))
    }
}

fn parse_lowercase_hex(text: &str) -> Option<[u8; 32]> {
    let is_lowercase_hex = text
        .bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    if text.len() != 64 || !is_lowercase_hex {
        return None;
    }
    let mut digest = [0_u8; 32];
    for (byte, pair) in digest
        .iter_mut()
        .zip(text.as_bytes().chunks(2))
    {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(digest)
}

/// Where a client caches a record: the registry's authority and the record's
/// key, and for a caller-gated record the principal that fetched it.
///
/// Records of two registries never share an entry, even for one PURL, and a
/// caller-gated record cached for one principal is a miss for another.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CacheKey {
    /// The registry that logged the record.
    pub authority: Authority,
    /// The record.
    pub record: RecordKey,
    /// The principal a caller-gated record was fetched for.
    pub principal: Option<String>,
}

impl CacheKey {
    /// A file-name-safe digest of the key: the lowercase hex SHA-256 of its
    /// fields, each prefixed with its length.
    #[must_use]
    pub fn file_name(&self) -> String {
        let (identity, tag) = match &self.record.identity {
            RecordIdentity::Package(purl) => (purl.as_str(), ""),
            RecordIdentity::OciTag { repository, tag } => (repository.as_str(), tag.as_str()),
        };
        let revision = self.record.revision.map(|revision| revision.to_string());
        let fields = [
            &self.authority.to_string(),
            identity,
            tag,
            revision.as_deref().unwrap_or("-"),
            self.record.profile.as_str(),
            self.principal.as_deref().unwrap_or("-"),
        ];
        let mut hasher = Sha256::new();
        for field in fields {
            hasher.update((field.len() as u64).to_be_bytes());
            hasher.update(field);
        }
        lowercase_hex(&hasher.finalize())
    }
}

#[cfg(test)]
mod tests;
