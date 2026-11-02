//! The TUF metadata fields this client reads.

use super::TufError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use std::collections::BTreeMap;

/// Signed metadata: the raw `signed` object, which signatures cover in its
/// canonical form, and the typed fields read from it.
pub(super) struct Signed<Body> {
    pub(super) body: Body,
    pub(super) value: Value,
    pub(super) signatures: Vec<RawSignature>,
}

#[derive(Deserialize)]
struct Envelope {
    signatures: Vec<RawSignature>,
    signed: Value,
}

#[derive(Clone, Deserialize)]
pub(super) struct RawSignature {
    pub(super) keyid: String,
    pub(super) sig: String,
}

/// The fields every role's metadata has.
#[derive(Clone, Deserialize)]
pub(super) struct Header {
    #[serde(rename = "_type")]
    role_type: String,
    spec_version: String,
    pub(super) version: u64,
    expires: String,
}

impl Header {
    /// Fails if the metadata expired at `now`.
    pub(super) fn check_expiry(
        &self,
        role: &'static str,
        now: DateTime<Utc>,
    ) -> Result<(), TufError> {
        let expires = DateTime::parse_from_rfc3339(&self.expires)
            .map_err(|_| TufError::InvalidExpiry { role })?;
        if now >= expires {
            return Err(TufError::Expired { role });
        }
        Ok(())
    }
}

/// A role's metadata body.
pub(super) trait RoleMetadata: DeserializeOwned {
    /// The role, which is also its `_type`.
    const ROLE: &'static str;

    fn header(&self) -> &Header;
}

#[derive(Clone, Deserialize)]
pub(super) struct Key {
    pub(super) keytype: String,
    pub(super) scheme: String,
    pub(super) keyval: KeyValue,
}

#[derive(Clone, Deserialize)]
pub(super) struct KeyValue {
    pub(super) public: String,
}

#[derive(Clone, PartialEq, Eq, Deserialize)]
pub(super) struct RoleKeys {
    pub(super) keyids: Vec<String>,
    pub(super) threshold: u64,
}

#[derive(Clone, Deserialize)]
pub(super) struct Root {
    #[serde(flatten)]
    pub(super) header: Header,
    pub(super) keys: BTreeMap<String, Key>,
    pub(super) roles: BTreeMap<String, RoleKeys>,
}

/// A file listed in timestamp or snapshot metadata.
#[derive(Clone, Deserialize)]
pub(super) struct MetaFile {
    pub(super) version: u64,
    pub(super) length: Option<u64>,
    #[serde(default)]
    pub(super) hashes: BTreeMap<String, String>,
}

/// Timestamp and snapshot metadata, which both list files by version.
#[derive(Clone, Deserialize)]
pub(super) struct FileList {
    #[serde(flatten)]
    pub(super) header: Header,
    pub(super) meta: BTreeMap<String, MetaFile>,
}

#[derive(Clone, Deserialize)]
pub(super) struct Timestamp(pub(super) FileList);

#[derive(Clone, Deserialize)]
pub(super) struct Snapshot(pub(super) FileList);

#[derive(Clone, Deserialize)]
pub(super) struct TargetFile {
    pub(super) length: u64,
    pub(super) hashes: BTreeMap<String, String>,
}

#[derive(Clone, Deserialize)]
pub(super) struct Targets {
    #[serde(flatten)]
    pub(super) header: Header,
    pub(super) targets: BTreeMap<String, TargetFile>,
}

impl RoleMetadata for Root {
    const ROLE: &'static str = "root";

    fn header(&self) -> &Header {
        &self.header
    }
}

impl RoleMetadata for Timestamp {
    const ROLE: &'static str = "timestamp";

    fn header(&self) -> &Header {
        &self.0.header
    }
}

impl RoleMetadata for Snapshot {
    const ROLE: &'static str = "snapshot";

    fn header(&self) -> &Header {
        &self.0.header
    }
}

impl RoleMetadata for Targets {
    const ROLE: &'static str = "targets";

    fn header(&self) -> &Header {
        &self.header
    }
}

/// Reads `bytes` as signed metadata of the role `Body` describes, checking
/// its type and specification version. Signatures are checked separately.
pub(super) fn parse<Body: RoleMetadata>(bytes: &[u8]) -> Result<Signed<Body>, TufError> {
    let envelope: Envelope = serde_json::from_slice(bytes)?;
    let body: Body = serde_json::from_value(envelope.signed.clone())?;
    let header = body.header();
    if header.role_type != Body::ROLE {
        return Err(TufError::WrongType { expected: Body::ROLE, actual: header.role_type.clone() });
    }
    if header.spec_version.split('.').next() != Some("1") {
        let version = header.spec_version.clone();
        return Err(TufError::UnsupportedSpecVersion { version });
    }
    Ok(Signed { body, value: envelope.signed, signatures: envelope.signatures })
}
