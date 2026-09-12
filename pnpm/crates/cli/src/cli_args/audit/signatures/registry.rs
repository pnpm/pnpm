use super::{
    BTreeMap, Config, Deserialize, HashMap, SignaturesError, ThrottledClient, encode_package_name,
    redact_url_credentials, retry_opts_from_config, sanitize_response_body, send_with_retry,
};

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RegistryKey {
    #[serde(default)]
    pub(super) expires: Option<String>,
    pub(super) key: String,
    pub(super) keyid: String,
    pub(super) keytype: String,
    pub(super) scheme: String,
}

#[derive(Debug, Deserialize)]
struct RegistryKeysResponse {
    keys: Vec<RegistryKey>,
}

#[derive(Debug, Deserialize)]
pub(super) struct PackageSignature {
    pub(super) keyid: String,
    pub(super) sig: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct Dist {
    #[serde(default)]
    pub(super) integrity: Option<String>,
    #[serde(default)]
    pub(super) tarball: Option<String>,
    #[serde(default)]
    pub(super) signatures: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub(super) struct PackumentVersion {
    #[serde(default)]
    pub(super) dist: Option<Dist>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Packument {
    /// Per-version publish times. Kept as raw JSON values (rather than
    /// `String`s) because the object also holds `created`/`modified` keys and
    /// pnpm never validates the shape — only `versions` is required.
    #[serde(default)]
    pub(super) time: BTreeMap<String, serde_json::Value>,
    pub(super) versions: HashMap<String, PackumentVersion>,
}

/// Parse an ISO-8601 / RFC-3339 timestamp to epoch milliseconds, returning
/// `None` when it can't be parsed (mirroring JS `Date.parse` yielding `NaN`,
/// which then compares false).
pub(super) fn parse_timestamp(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value).ok().map(|datetime| datetime.timestamp_millis())
}

pub(super) async fn fetch_registry_keys(
    registry: &str,
    config: &Config,
    http_client: &ThrottledClient,
) -> Result<Vec<RegistryKey>, SignaturesError> {
    let registry_url = with_trailing_slash(registry);
    let keys_url = format!("{registry_url}-/npm/v1/keys");
    let display_url = redact_url_credentials(&keys_url);
    let authorization = config.auth_headers.for_url(&registry_url);
    // Keep the throttle guard alive until the body is fully read; dropping it
    // before `response.text()` would release the concurrency permit while the
    // socket is still draining (see [`send_with_retry`]).
    let (_guard, response) =
        send_with_retry(http_client, &keys_url, retry_opts_from_config(config), |client| {
            let mut request = client.get(&keys_url).header("accept", "application/json");
            if let Some(value) = &authorization {
                request = request.header("authorization", value);
            }
            request
        })
        .await
        .map_err(|source| SignaturesError::KeysNetwork {
            url: display_url.clone(),
            reason: redact_url_credentials(&source.to_string()),
        })?;

    let status = response.status().as_u16();
    let body = response.text().await.map_err(|source| SignaturesError::KeysNetwork {
        url: display_url.clone(),
        reason: redact_url_credentials(&source.to_string()),
    })?;
    // npm registries answer 404 (no signing) and 400 the same way: there is no
    // trust root, so the registry's packages are simply not audited.
    if status == 404 || status == 400 {
        return Ok(Vec::new());
    }
    if status != 200 {
        return Err(SignaturesError::KeysBadStatus {
            url: display_url,
            status,
            body: sanitize_response_body(&body),
        });
    }

    parse_registry_keys(&body, &display_url)
}

/// The registry's signing keys. npm registry signing uses ECDSA P-256
/// keys; provenance attestations are handled separately and intentionally
/// ignored here.
fn parse_registry_keys(body: &str, display_url: &str) -> Result<Vec<RegistryKey>, SignaturesError> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|err| SignaturesError::KeysInvalidJson {
            url: display_url.to_string(),
            reason: err.to_string(),
            body: sanitize_response_body(body),
        })?;
    let parsed: RegistryKeysResponse =
        serde_json::from_value(value.clone()).map_err(|_| SignaturesError::KeysUnexpectedBody {
            url: display_url.to_string(),
            body: sanitize_response_body(&value.to_string()),
        })?;
    Ok(parsed
        .keys
        .into_iter()
        .filter(|key| key.keytype == "ecdsa-sha2-nistp256" && key.scheme == "ecdsa-sha2-nistp256")
        .collect())
}

pub(super) async fn fetch_packument(
    name: &str,
    registry: &str,
    config: &Config,
    http_client: &ThrottledClient,
) -> Result<Option<Packument>, SignaturesError> {
    let registry_url = with_trailing_slash(registry);
    let packument_url = format!("{registry_url}{}", encode_package_name(name));
    let display_url = redact_url_credentials(&packument_url);
    let authorization = config.auth_headers.for_url(&registry_url);
    // Hold the throttle guard until the body is read; see `fetch_registry_keys`.
    let (_guard, response) =
        send_with_retry(http_client, &packument_url, retry_opts_from_config(config), |client| {
            let mut request = client.get(&packument_url).header("accept", "application/json");
            if let Some(value) = &authorization {
                request = request.header("authorization", value);
            }
            request
        })
        .await
        .map_err(|source| SignaturesError::PackumentNetwork {
            url: display_url.clone(),
            reason: redact_url_credentials(&source.to_string()),
        })?;

    let status = response.status().as_u16();
    let body = response.text().await.map_err(|source| SignaturesError::PackumentNetwork {
        url: display_url.clone(),
        reason: redact_url_credentials(&source.to_string()),
    })?;
    if status == 404 {
        return Ok(None);
    }
    if status != 200 {
        return Err(SignaturesError::PackumentBadStatus {
            url: display_url,
            status,
            body: sanitize_response_body(&body),
        });
    }

    parse_packument(&body, &display_url).map(Some)
}

fn parse_packument(body: &str, display_url: &str) -> Result<Packument, SignaturesError> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|err| SignaturesError::PackumentInvalidJson {
            url: display_url.to_string(),
            reason: err.to_string(),
            body: sanitize_response_body(body),
        })?;
    serde_json::from_value(value.clone()).map_err(|_| SignaturesError::PackumentUnexpectedBody {
        url: display_url.to_string(),
        body: sanitize_response_body(&value.to_string()),
    })
}

fn with_trailing_slash(registry: &str) -> String {
    if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") }
}
