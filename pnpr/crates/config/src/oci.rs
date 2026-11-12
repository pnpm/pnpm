use std::num::NonZeroU64;

use pnpr_error::RegistryError;
use serde::Deserialize;

use crate::byte_size::{ByteSize, parse_positive_byte_size};

/// Authentication and byte limits for the OCI distribution surface.
#[derive(Debug, Clone)]
pub struct OciConfig {
    pub bearer_auth: bool,
    pub max_blob_bytes: u64,
    /// Upload chunk size hint advertised to clients. `None` advertises none.
    pub chunk_max_bytes: Option<NonZeroU64>,
    pub max_manifest_bytes: usize,
}

impl OciConfig {
    /// 10 GiB.
    pub const DEFAULT_MAX_BLOB_BYTES: u64 = 10 * 1024 * 1024 * 1024;
    /// 4 MiB.
    pub const DEFAULT_MAX_MANIFEST_BYTES: usize = 4 * 1024 * 1024;
}

impl Default for OciConfig {
    fn default() -> Self {
        Self {
            bearer_auth: false,
            max_blob_bytes: Self::DEFAULT_MAX_BLOB_BYTES,
            chunk_max_bytes: None,
            max_manifest_bytes: Self::DEFAULT_MAX_MANIFEST_BYTES,
        }
    }
}

/// Disk shape of the `oci:` block.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OciFile {
    bearer_auth: bool,
    max_blob_size: Option<ByteSize>,
    max_chunk_size: Option<ByteSize>,
    max_manifest_size: Option<ByteSize>,
}

pub(crate) fn build_oci_config(file: &OciFile) -> Result<OciConfig, RegistryError> {
    let default = OciConfig::default();
    let max_blob_bytes = match &file.max_blob_size {
        Some(raw) => parse_positive_byte_size("oci.maxBlobSize", raw)?,
        None => default.max_blob_bytes,
    };
    let chunk_max_bytes = file.max_chunk_size
        .as_ref()
        .map(|raw| parse_positive_byte_size("oci.maxChunkSize", raw))
        .transpose()?
        .and_then(NonZeroU64::new);
    let max_manifest_bytes = match &file.max_manifest_size {
        Some(raw) => usize::try_from(parse_positive_byte_size("oci.maxManifestSize", raw)?)
            .map_err(|_| RegistryError::InvalidConfig {
                reason: "oci.maxManifestSize is too large".to_string(),
            })?,
        None => default.max_manifest_bytes,
    };
    Ok(OciConfig {
        bearer_auth: file.bearer_auth,
        max_blob_bytes,
        chunk_max_bytes,
        max_manifest_bytes,
    })
}
