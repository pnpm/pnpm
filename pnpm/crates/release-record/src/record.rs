//! A record's bytes and the core fields every profile shares.

use crate::{
    digest::RecordDigest,
    identity::{LogEntry, LogEntryError, RecordIdentity, RecordKey},
    profile::Profile,
    purl::{Purl, PurlError},
};
use derive_more::{Display, Error};
use serde::{Deserialize, Deserializer};

/// The `schema` of every record this crate reads.
pub const RECORD_SCHEMA: &str = "pnpr-release-record/1";

/// A record as the registry stored it: its exact bytes, their digest, and
/// the core fields read from them.
///
/// The bytes are what the log entry and every statement commit to, through
/// the digest. They are never re-serialized: an auditor that re-derives a
/// record compares its manifest under the profile, not its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRecord {
    bytes: Vec<u8>,
    digest: RecordDigest,
    key: RecordKey,
    image: Option<Purl>,
}

/// Why record bytes were rejected.
#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum RecordError {
    #[display("record is not valid JSON: {_0}")]
    Json(serde_json::Error),
    #[display("unsupported record schema {schema}")]
    UnsupportedSchema {
        #[error(not(source))]
        schema: String,
    },
    #[display("{profile} record is missing {field}")]
    MissingField {
        #[error(not(source))]
        profile: Profile,
        #[error(not(source))]
        field: &'static str,
    },
    #[display("{profile} record has {field}, which its profile does not")]
    UnexpectedField {
        #[error(not(source))]
        profile: Profile,
        #[error(not(source))]
        field: &'static str,
    },
    #[display("OCI tag record's package URL {purl} is not the image of its repository and tag")]
    ImageMismatch {
        #[error(not(source))]
        purl: String,
    },
    Purl(PurlError),
    #[display("{profile} record has a {purl_type} package URL")]
    WrongPurlType {
        #[error(not(source))]
        profile: Profile,
        #[error(not(source))]
        purl_type: String,
    },
    #[display("record digest is {actual}, expected {expected}")]
    DigestMismatch {
        expected: RecordDigest,
        actual: RecordDigest,
    },
    LogEntry(LogEntryError),
}

#[derive(Deserialize)]
struct Core {
    schema: String,
    #[serde(default, deserialize_with = "present")]
    purl: Option<Option<Purl>>,
    revision: Option<u64>,
    interpretation: Profile,
    repository: Option<String>,
    tag: Option<String>,
}

/// Tells a `null` field (`Some(None)`) apart from a missing one (`None`).
fn present<'de, Value: Deserialize<'de>, Source: Deserializer<'de>>(
    deserializer: Source,
) -> Result<Option<Option<Value>>, Source::Error> {
    Option::deserialize(deserializer).map(Some)
}

impl StoredRecord {
    /// Reads the core fields of `bytes`, which must be the bytes the
    /// registry stored, after any content encoding is decoded.
    ///
    /// A record of a known profile must carry the fields its profile
    /// requires; an OCI tag record must name its repository, tag, and
    /// revision, and has a `purl` that is `null` for a tombstone.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, RecordError> {
        let digest = RecordDigest::of(&bytes);
        StoredRecord::parse_with_digest(bytes, digest)
    }

    /// Reads a record whose digest a lockfile or log entry already names,
    /// rejecting any other bytes.
    pub fn parse_expecting(bytes: Vec<u8>, expected: RecordDigest) -> Result<Self, RecordError> {
        let actual = RecordDigest::of(&bytes);
        if actual != expected {
            return Err(RecordError::DigestMismatch { expected, actual });
        }
        StoredRecord::parse_with_digest(bytes, actual)
    }

    fn parse_with_digest(bytes: Vec<u8>, digest: RecordDigest) -> Result<Self, RecordError> {
        let core: Core = serde_json::from_slice(&bytes).map_err(RecordError::Json)?;
        if core.schema != RECORD_SCHEMA {
            return Err(RecordError::UnsupportedSchema { schema: core.schema });
        }
        let (key, image) = record_key(core)?;
        Ok(StoredRecord { bytes, digest, key, image })
    }

    /// The record's bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The SHA-512 of the record's bytes.
    #[must_use]
    pub const fn digest(&self) -> &RecordDigest {
        &self.digest
    }

    /// The record's key: its identity, revision, and profile.
    #[must_use]
    pub const fn key(&self) -> &RecordKey {
        &self.key
    }

    /// For an OCI tag record, the image the tag points at; `None` for a
    /// tombstone and for records of other profiles.
    #[must_use]
    pub const fn image(&self) -> Option<&Purl> {
        self.image.as_ref()
    }

    /// The log entry that commits to this record.
    pub fn log_entry(&self) -> Result<LogEntry, RecordError> {
        LogEntry::new(self.key.clone(), self.digest).map_err(RecordError::LogEntry)
    }
}

fn record_key(core: Core) -> Result<(RecordKey, Option<Purl>), RecordError> {
    let profile = core.interpretation;
    let missing = |field| RecordError::MissingField { profile: profile.clone(), field };
    if profile.is_oci_tag() {
        let repository = core.repository.ok_or_else(|| missing("repository"))?;
        let tag = core.tag.ok_or_else(|| missing("tag"))?;
        let revision = core.revision.ok_or_else(|| missing("revision"))?;
        let image = core.purl.ok_or_else(|| missing("purl"))?;
        if let Some(image) = &image {
            check_image(image, &repository, &tag)?;
        }
        let identity = RecordIdentity::OciTag { repository, tag };
        return Ok((RecordKey { identity, revision: Some(revision), profile }, image));
    }
    match (profile.has_revisions(), core.revision) {
        (Some(true), None) => return Err(missing("revision")),
        (Some(false), Some(_)) => {
            return Err(RecordError::UnexpectedField { profile, field: "revision" });
        }
        _ => {}
    }
    let purl = core.purl.flatten().ok_or_else(|| missing("purl"))?;
    if profile
        .purl_type()
        .is_some_and(|expected| expected != purl.purl_type())
    {
        let purl_type = purl.purl_type().to_owned();
        return Err(RecordError::WrongPurlType { profile, purl_type });
    }
    purl.check_canonical().map_err(RecordError::Purl)?;
    let identity = RecordIdentity::Package(purl);
    Ok((RecordKey { identity, revision: core.revision, profile }, None))
}

/// Checks that an OCI tag record's `purl` is the one [`Purl::oci_tag`]
/// builds from its repository, its tag, and the manifest digest it names, so
/// the image cannot disagree with the tag the record binds.
fn check_image(image: &Purl, repository: &str, tag: &str) -> Result<(), RecordError> {
    let mismatch = || RecordError::ImageMismatch { purl: image.to_string() };
    let digest = image
        .as_str()
        .split_once('@')
        .and_then(|(_, rest)| rest.split('?').next())
        .ok_or_else(mismatch)?;
    let expected = Purl::oci_tag(repository, digest, tag).map_err(|_| mismatch())?;
    if &expected != image {
        return Err(mismatch());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
