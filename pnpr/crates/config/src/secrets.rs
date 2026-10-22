use std::sync::Arc;

use pnpm_env_replace::{EnvVar, env_replace_lossy};
use pnpr_error::RegistryError;
use sha2::{Digest, Sha256};

use crate::config_file::SecretsFile;

/// Minimum length of an explicit `secret:` in bytes.
pub const MIN_RESOLUTION_SECRET_LEN: usize = 16;

/// Derive a domain-separated subkey from a master secret using SHA-256 (`purpose \0 root_secret`).
#[must_use]
pub fn derive_subkey(root_secret: &[u8], purpose: &[u8]) -> Arc<[u8]> {
    let mut hasher = Sha256::new();
    hasher.update(purpose);
    hasher.update(b"\0");
    hasher.update(root_secret);
    Arc::from(hasher.finalize().to_vec())
}

/// The HMAC secret keying private resolution-cache entries: the YAML
/// `secret:` when set (rejected if too short to be a safe HMAC key), else a
/// fresh per-process value.
pub fn resolution_secret(secret: Option<&str>) -> Result<Arc<[u8]>, RegistryError> {
    match secret {
        Some(secret) => {
            validate_secret_len("secret:", secret)?;
            Ok(Arc::from(secret.as_bytes().to_vec()))
        }
        None => Ok(random_secret()),
    }
}

/// Resolved cache and OCI secrets with configuration status.
pub(crate) struct ResolvedSecrets {
    pub(crate) cache: Arc<[u8]>,
    pub(crate) oci: Arc<[u8]>,
    pub(crate) has_configured: bool,
}

fn validate_secret_len(field: &str, secret: &str) -> Result<(), RegistryError> {
    if secret.len() < MIN_RESOLUTION_SECRET_LEN {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "`{field}` must be at least {MIN_RESOLUTION_SECRET_LEN} bytes (it is {})",
                secret.len(),
            ),
        });
    }
    Ok(())
}

fn resolve_cache_secret(
    master: Option<&[u8]>,
    secrets: Option<&SecretsFile>,
) -> Result<Arc<[u8]>, RegistryError> {
    if let Some(cache) = secrets.and_then(|file| file.cache.as_deref()) {
        validate_secret_len("secrets.cache", cache)?;
        return Ok(Arc::from(cache.as_bytes().to_vec()));
    }
    match master {
        Some(master_bytes) => Ok(Arc::from(master_bytes.to_vec())),
        None => Ok(random_secret()),
    }
}

fn resolve_oci_secret(
    master: Option<&[u8]>,
    secrets: Option<&SecretsFile>,
) -> Result<Arc<[u8]>, RegistryError> {
    if let Some(oci) = secrets.and_then(|file| file.oci.as_deref()) {
        validate_secret_len("secrets.oci", oci)?;
        return Ok(Arc::from(oci.as_bytes().to_vec()));
    }
    let base = master.map_or_else(random_secret, |bytes| Arc::from(bytes.to_vec()));
    Ok(derive_subkey(&base, b"pnpr OCI token signing v1"))
}

/// Resolve the cache and OCI secrets from optional master and purpose-specific secret blocks.
pub(crate) fn resolve_secrets(
    master_secret: Option<&str>,
    secrets: Option<&SecretsFile>,
) -> Result<ResolvedSecrets, RegistryError> {
    let has_master = master_secret.is_some();
    let has_configured = has_master || secrets.is_some_and(|file| file.cache.is_some());

    let master_bytes = match master_secret {
        Some(secret) => {
            validate_secret_len("secret:", secret)?;
            Some(secret.as_bytes())
        }
        None => None,
    };

    let cache = resolve_cache_secret(master_bytes, secrets)?;
    let oci = resolve_oci_secret(master_bytes, secrets)?;
    Ok(ResolvedSecrets { cache, oci, has_configured })
}

/// 32 bytes from the OS CSPRNG, for a deployment that configures no
/// `secret:`. Private cache entries then live only for this process.
#[must_use]
pub fn random_secret() -> Arc<[u8]> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("OS CSPRNG must be available");
    Arc::from(bytes.to_vec())
}

fn validate_root_secrets<Sys: EnvVar>(val: &serde_json::Value) -> Result<(), RegistryError> {
    if let Some(secret) = val.get("secret").and_then(serde_json::Value::as_str) {
        check_secret_env_field::<Sys>("secret", secret)?;
    }
    let Some(secrets) = val.get("secrets").and_then(serde_json::Value::as_object) else {
        return Ok(());
    };
    if let Some(cache) = secrets.get("cache").and_then(serde_json::Value::as_str) {
        check_secret_env_field::<Sys>("secrets.cache", cache)?;
    }
    if let Some(oci) = secrets.get("oci").and_then(serde_json::Value::as_str) {
        check_secret_env_field::<Sys>("secrets.oci", oci)?;
    }
    Ok(())
}

fn validate_s3_secrets<Sys: EnvVar>(val: &serde_json::Value) -> Result<(), RegistryError> {
    let Some(s3) = val.get("s3").and_then(serde_json::Value::as_object) else {
        return Ok(());
    };
    for key in ["secretAccessKey", "secret_access_key"] {
        if let Some(secret) = s3.get(key).and_then(serde_json::Value::as_str) {
            check_secret_env_field::<Sys>(&format!("s3.{key}"), secret)?;
        }
    }
    Ok(())
}

fn validate_auth_secrets<Sys: EnvVar>(val: &serde_json::Value) -> Result<(), RegistryError> {
    if let Some(token) = val.pointer("/auth/scim/token").and_then(serde_json::Value::as_str) {
        check_secret_env_field::<Sys>("auth.scim.token", token)?;
    }
    let Some(oidc) = val.pointer("/auth/oidc").and_then(serde_json::Value::as_array) else {
        return Ok(());
    };
    for (index, provider) in oidc.iter().enumerate() {
        let client_secret = provider
            .pointer("/login/clientSecret")
            .or_else(|| provider.pointer("/login/client_secret"))
            .and_then(serde_json::Value::as_str);
        if let Some(secret) = client_secret {
            check_secret_env_field::<Sys>(
                &format!("auth.oidc[{index}].login.clientSecret"),
                secret,
            )?;
        }
    }
    Ok(())
}

fn validate_database_secrets<Sys: EnvVar>(val: &serde_json::Value) -> Result<(), RegistryError> {
    for prefix in ["/backend/libsql", "/auth/libsql", "/storage/database"] {
        for field in ["authToken", "auth_token"] {
            let ptr = format!("{prefix}/{field}");
            if let Some(token) = val.pointer(&ptr).and_then(serde_json::Value::as_str) {
                check_secret_env_field::<Sys>(&ptr[1..], token)?;
            }
        }
    }
    Ok(())
}

fn check_registry_auth<Sys: EnvVar>(
    path: &str,
    auth: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), RegistryError> {
    for key in ["token", "bearerToken", "bearer_token"] {
        if let Some(token) = auth.get(key).and_then(serde_json::Value::as_str) {
            check_secret_env_field::<Sys>(&format!("{path}.auth.{key}"), token)?;
        }
    }
    Ok(())
}

fn validate_single_registry_secrets<Sys: EnvVar>(
    path: &str,
    reg: &serde_json::Value,
) -> Result<(), RegistryError> {
    let auth = reg
        .get("auth")
        .or_else(|| reg.pointer("/upstream/auth"))
        .and_then(serde_json::Value::as_object);
    if let Some(auth_map) = auth {
        check_registry_auth::<Sys>(path, auth_map)?;
    }
    if let Some(headers) = reg.get("headers").and_then(serde_json::Value::as_object) {
        for (hdr, val) in headers {
            if let Some(val_str) = val.as_str() {
                check_secret_env_field::<Sys>(&format!("{path}.headers.{hdr}"), val_str)?;
            }
        }
    }
    Ok(())
}

fn is_flat_registry(reg_map: &serde_json::Map<String, serde_json::Value>) -> bool {
    reg_map
        .get("type")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|typ| matches!(typ, "hosted" | "upstream" | "router"))
        || reg_map
            .get("url")
            .and_then(serde_json::Value::as_str)
            .is_some()
}

fn validate_registry_secrets<Sys: EnvVar>(val: &serde_json::Value) -> Result<(), RegistryError> {
    let Some(registries) = val.get("registries").and_then(serde_json::Value::as_object) else {
        return Ok(());
    };
    for (name, entry) in registries {
        let Some(reg_map) = entry.as_object() else {
            continue;
        };
        if is_flat_registry(reg_map) {
            validate_single_registry_secrets::<Sys>(&format!("registries.{name}"), entry)?;
        } else {
            for (sub_name, sub_entry) in reg_map {
                validate_single_registry_secrets::<Sys>(
                    &format!("registries.{name}.{sub_name}"),
                    sub_entry,
                )?;
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_secret_env_vars<Sys: EnvVar>(raw: &str) -> Result<(), RegistryError> {
    let Ok(val) = serde_saphyr::from_str::<serde_json::Value>(raw) else {
        return Ok(());
    };
    validate_root_secrets::<Sys>(&val)?;
    validate_s3_secrets::<Sys>(&val)?;
    validate_auth_secrets::<Sys>(&val)?;
    validate_database_secrets::<Sys>(&val)?;
    validate_registry_secrets::<Sys>(&val)?;
    Ok(())
}

fn check_secret_env_field<Sys: EnvVar>(field: &str, raw_val: &str) -> Result<(), RegistryError> {
    let (_, unresolved) = env_replace_lossy::<Sys>(raw_val);
    if !unresolved.is_empty() {
        return Err(RegistryError::InvalidConfig {
            reason: format!(
                "secret-bearing configuration field `{field}` references unset environment variable: {unresolved:?}",
            ),
        });
    }
    Ok(())
}
