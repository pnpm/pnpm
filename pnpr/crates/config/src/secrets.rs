use std::sync::Arc;

use pnpr_error::RegistryError;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::secret::{Secret, refuse};

/// Minimum length of a configured `secret:` in bytes.
pub const MIN_SECRET_LEN: usize = 16;

/// Marker file in `cache_dir` recording the one-way fingerprint of the cache
/// secret that initialized the directory.
pub const CACHE_SECRET_MARKER: &str = ".cache-secret";

const CACHE_KEY_PURPOSE: &[u8] = b"pnpr cache v1";
const OCI_KEY_PURPOSE: &[u8] = b"pnpr OCI token signing v1";

/// Derive a domain-separated subkey from a master secret using SHA-256 (`purpose \0 root_secret`).
#[must_use]
pub fn derive_subkey(root_secret: &[u8], purpose: &[u8]) -> Arc<[u8]> {
    let mut hasher = Sha256::new();
    hasher.update(purpose);
    hasher.update(b"\0");
    hasher.update(root_secret);
    Arc::from(hasher.finalize().to_vec())
}

/// One-way hex fingerprint of `cache_secret`, safe to store in
/// [`CACHE_SECRET_MARKER`] without exposing secret material.
#[must_use]
pub fn cache_secret_fingerprint(cache_secret: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = derive_subkey(cache_secret, b"pnpr cache secret fingerprint v1");
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest.iter() {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// 32 bytes from the OS CSPRNG, for a deployment that configures no
/// `secret:`. Private cache entries then live only for this process.
#[must_use]
pub fn random_secret() -> Arc<[u8]> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("OS CSPRNG must be available");
    Arc::from(bytes.to_vec())
}

/// Resolved cache and OCI keys with configuration status.
pub(crate) struct ResolvedSecrets {
    pub(crate) cache: Arc<[u8]>,
    pub(crate) oci: Arc<[u8]>,
    pub(crate) has_configured: bool,
}

/// The `secret:` value: one root for every purpose, or a root per purpose so
/// each rotates alone.
#[derive(Debug)]
pub(crate) enum SecretFile {
    Root(Secret),
    Keys(SecretKeysFile),
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SecretKeysFile {
    #[serde(default)]
    cache: Option<Secret>,
    #[serde(default)]
    oci: Option<Secret>,
}

impl<'de> Deserialize<'de> for SecretFile {
    fn deserialize<De>(deserializer: De) -> Result<Self, De::Error>
    where
        De: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(SecretFileVisitor)
    }
}

struct SecretFileVisitor;

impl<'de> serde::de::Visitor<'de> for SecretFileVisitor {
    type Value = SecretFile;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a secret string or a table of `cache` and `oci` secrets")
    }

    fn visit_i64<DeError: serde::de::Error>(self, _: i64) -> Result<SecretFile, DeError> {
        Err(not_a_string())
    }

    fn visit_u64<DeError: serde::de::Error>(self, _: u64) -> Result<SecretFile, DeError> {
        Err(not_a_string())
    }

    fn visit_f64<DeError: serde::de::Error>(self, _: f64) -> Result<SecretFile, DeError> {
        Err(not_a_string())
    }

    fn visit_bool<DeError: serde::de::Error>(self, _: bool) -> Result<SecretFile, DeError> {
        Err(not_a_string())
    }

    fn visit_str<DeError: serde::de::Error>(self, value: &str) -> Result<SecretFile, DeError> {
        Secret::checked(value.to_string()).map(SecretFile::Root).map_err(DeError::custom)
    }

    fn visit_map<Map: serde::de::MapAccess<'de>>(self, map: Map) -> Result<SecretFile, Map::Error> {
        SecretKeysFile::deserialize(serde::de::value::MapAccessDeserializer::new(map))
            .map(SecretFile::Keys)
    }
}

/// YAML reads an unquoted all-digit or `true`/`false` value as a number or a
/// boolean, which loses its exact text, such as leading zeros.
fn not_a_string<DeError: serde::de::Error>() -> DeError {
    DeError::custom(refuse(
        "`secret` reads as a number or a boolean; quote it, as in `secret: \"${PNPR_SECRET}\"`"
            .to_string(),
    ))
}

fn validate_secret_len(field: &str, secret: &Secret) -> Result<(), RegistryError> {
    if secret.len() < MIN_SECRET_LEN {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "`{field}` must be at least {MIN_SECRET_LEN} bytes (it is {})",
                secret.len(),
            ),
        });
    }
    Ok(())
}

/// The key for `purpose`, derived from its configured root, or from a
/// per-process random one.
fn purpose_key(root: Option<&Secret>, purpose: &[u8]) -> Arc<[u8]> {
    match root {
        Some(root) => derive_subkey(root.as_bytes(), purpose),
        None => derive_subkey(&random_secret(), purpose),
    }
}

/// The cache and OCI keys `secret:` configures.
pub(crate) fn resolve_secrets(
    secret: Option<&SecretFile>,
) -> Result<ResolvedSecrets, RegistryError> {
    let (cache_root, oci_root) = match secret {
        None => (None, None),
        Some(SecretFile::Root(root)) => {
            validate_secret_len("secret", root)?;
            (Some(root), Some(root))
        }
        Some(SecretFile::Keys(keys)) => {
            for (field, key) in [("secret.cache", &keys.cache), ("secret.oci", &keys.oci)] {
                if let Some(key) = key {
                    validate_secret_len(field, key)?;
                }
            }
            (keys.cache.as_ref(), keys.oci.as_ref())
        }
    };
    Ok(ResolvedSecrets {
        cache: purpose_key(cache_root, CACHE_KEY_PURPOSE),
        oci: purpose_key(oci_root, OCI_KEY_PURPOSE),
        has_configured: cache_root.is_some(),
    })
}
