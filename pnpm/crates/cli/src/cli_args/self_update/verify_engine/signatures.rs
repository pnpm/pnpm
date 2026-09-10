use super::{
    Config, Deserialize, EngineComponent, RetryOpts, SelfUpdateError, Signature, ThrottledClient,
    VerifyingKey, classify_signature_failures, encode_package_name, redact_and_sanitize,
    send_with_retry,
};
use base64::Engine as _;
use p256::{ecdsa::signature::Verifier, pkcs8::DecodePublicKey};

/// npm's public registry signing keys, mirrored from
/// <https://registry.npmjs.org/-/npm/v1/keys>. `expires` is `None` for a
/// key with no expiry.
pub(super) const NPM_SIGNING_KEYS: &[NpmSigningKey] = &[
    NpmSigningKey {
        keyid: "SHA256:jl3bwswu80PjjokCgh0o2w5c2U4LhQAE57gj9cz1kzA",
        key: "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE1Olb3zMAFFxXKHiIkQO5cJ3Yhl5i6UPp+IhuteBJbuHcA5UogKo0EWtlWwW6KSaKoTNEYL7JlCQiVnkhBktUgg==",
        expires: Some("2025-01-29T00:00:00.000Z"),
    },
    NpmSigningKey {
        keyid: "SHA256:DhQ8wR5APBvFHLF/+Tc+AYvPOdTpcIDqOhxsBHRwC7U",
        key: "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEY6Ya7W++7aUPzvMTrezH6Ycx3c+HOKYCcNGybJZSCJq/fd7Qa8uuAKtdIkUQtQiEKERhAmE5lMMJhP8OkDOa2g==",
        expires: None,
    },
];

pub(super) struct NpmSigningKey<'a> {
    pub(super) keyid: &'a str,
    pub(super) key: &'a str,
    pub(super) expires: Option<&'a str>,
}

/// The canonical npm registry, consulted for signature metadata when a
/// user-configured registry cannot provide a verifiable signature. Where the
/// signature bytes come from does not affect what they prove: they are
/// verified against the embedded keys over the lockfile integrity, so a
/// component passes only when a genuine signature validates over the bytes
/// actually installed. See <https://github.com/pnpm/pnpm/issues/13147>.
pub(super) const CANONICAL_NPM_REGISTRY: &str = "https://registry.npmjs.org/";

#[derive(PartialEq, Eq)]
pub(super) enum FailureCategory {
    Invalid,
    Absent,
    Unreachable,
    /// The lockfile integrity is not a sha512 hash (e.g. a sha1 pin from a
    /// registry that publishes only `shasum`), so no npm registry signature
    /// can ever validate over it — verification is impossible by
    /// construction, not evidence of tampering.
    Uncovered,
}

pub(super) struct SignatureFailure {
    pub(super) label: String,
    pub(super) registry: String,
    pub(super) reason: String,
    pub(super) category: FailureCategory,
}

impl SignatureFailure {
    pub(super) fn describe(&self) -> String {
        format!("{}: {}", self.label, self.reason)
    }

    /// Whether the engine may run despite this failure: no signature was
    /// obtainable (nothing suspicious was observed — as opposed to a
    /// signature that exists but does not validate, or the canonical
    /// registry answering that no signed release exists), and the component
    /// resolves through a registry the user configured themselves.
    pub(super) fn tolerable_without_signature(&self) -> bool {
        matches!(self.category, FailureCategory::Unreachable | FailureCategory::Uncovered)
            && !equal_registries(&self.registry, CANONICAL_NPM_REGISTRY)
    }
}

/// Per-component verification. Returns `None` when a registry signature
/// validates over the lockfile bytes — from the component's own registry,
/// or from `fallback_registry` when its own registry cannot provide a
/// verifiable one. `fallback_registry` is [`CANONICAL_NPM_REGISTRY`] in
/// production and a mock registry in unit tests. The fallback is attempted
/// once because it is an optional availability check after the primary
/// registry already failed to provide a verifiable signature.
pub(super) async fn find_signature_failure(
    component: &EngineComponent,
    fallback_registry: &str,
    keys: &[NpmSigningKey<'_>],
    client: &ThrottledClient,
    retry_opts: RetryOpts,
    config: &Config,
) -> Option<SignatureFailure> {
    let label = format!("{}@{}", component.name, component.version);
    let failure = |reason: String, category: FailureCategory| {
        Some(SignatureFailure {
            reason,
            category,
            label: label.clone(),
            registry: redact_and_sanitize(&component.registry),
        })
    };

    // npm registry signatures sign `name@version:integrity` with the sha512
    // integrity the registry published; any other installed form can never
    // validate, and verifying it would misreport an authentic release as
    // tampered with.
    if !component.integrity.starts_with("sha512-") {
        return failure(
            format!(
                "{label} is pinned by a non-sha512 integrity, which npm registry signatures cannot cover",
            ),
            FailureCategory::Uncovered,
        );
    }

    let primary = attempt_signature_verification(
        component,
        &component.registry,
        keys,
        client,
        retry_opts,
        config,
    )
    .await?;
    if equal_registries(&component.registry, fallback_registry) {
        return failure(primary.0, primary.1);
    }

    // A genuine signature validating over the installed integrity proves the
    // installed bytes regardless of which registry the primary attempt hit
    // or what it answered (e.g. a mirror serving stale signatures from a
    // rotated key), so a fallback pass is a pass.
    let secondary = attempt_signature_verification(
        component,
        fallback_registry,
        keys,
        client,
        RetryOpts { retries: 0, ..retry_opts },
        config,
    )
    .await?;

    classify_signature_failures(primary, secondary, fallback_registry, failure)
}

/// Verify `component`'s lockfile integrity against the signatures the
/// packument on `registry` carries. Returns `None` on success, otherwise
/// the failure reason and category.
async fn attempt_signature_verification(
    component: &EngineComponent,
    registry: &str,
    keys: &[NpmSigningKey<'_>],
    client: &ThrottledClient,
    retry_opts: RetryOpts,
    config: &Config,
) -> Option<(String, FailureCategory)> {
    let label = format!("{}@{}", component.name, component.version);
    // Registry URLs may carry inline `user:pass@` credentials, and the
    // reasons built here end up in error messages and warnings.
    let display_registry = redact_and_sanitize(registry);
    let packument = match fetch_packument(component, registry, client, retry_opts, config).await {
        Ok(Some(packument)) => packument,
        Ok(None) => {
            return Some((
                format!("{} is not published on {display_registry}", component.name),
                FailureCategory::Absent,
            ));
        }
        Err(reason) => return Some((reason, FailureCategory::Unreachable)),
    };

    let Some(version) = packument.versions.get(&component.version) else {
        return Some((
            format!("{label} was not found on {display_registry}"),
            FailureCategory::Absent,
        ));
    };
    let raw_signatures = version.dist.as_ref().and_then(|dist| dist.signatures.as_ref());
    let Some(parsed_signatures) = parse_signatures(raw_signatures) else {
        return Some((
            format!("malformed registry signatures metadata for {label}"),
            FailureCategory::Absent,
        ));
    };
    if parsed_signatures.is_empty() {
        return Some((
            format!("{label} has no registry signature on {display_registry}"),
            FailureCategory::Absent,
        ));
    }

    let published_at = packument.time.get(&component.version).and_then(serde_json::Value::as_str);
    // The message is built from the *lockfile* integrity, so a signature
    // only validates when the installed bytes match what the registry
    // signed.
    if signature_validates_against(component, &parsed_signatures, published_at, keys) {
        None
    } else {
        Some(("invalid registry signature".to_string(), FailureCategory::Invalid))
    }
}

/// The `dist.signatures` entries; empty when absent, `None` when malformed.
fn parse_signatures(raw: Option<&serde_json::Value>) -> Option<Vec<PackageSignature>> {
    match raw {
        None => Some(Vec::new()),
        Some(serde_json::Value::Array(elements)) => elements
            .iter()
            .map(|element| serde_json::from_value::<PackageSignature>(element.clone()).ok())
            .collect(),
        Some(_) => None,
    }
}

/// Whether two registry URLs address the same registry. URL-equivalent
/// forms must compare equal — hosts are case-insensitive and default ports
/// are implied — or a canonical registry written as e.g.
/// `https://Registry.NPMJS.org:443/` would be misclassified as a different,
/// non-canonical one, weakening fail-closed decisions keyed on whether the
/// registry is the canonical one. Inline `user:pass@` credentials are auth
/// material, not identity, so they are stripped before comparing for the
/// same reason.
fn equal_registries(left: &str, right: &str) -> bool {
    normalize_registry_url(left).eq_ignore_ascii_case(&normalize_registry_url(right))
}

fn normalize_registry_url(registry: &str) -> String {
    let with_slash = redact_and_sanitize(&with_trailing_slash(registry));
    // URL normalization lowercases the host and drops a default port.
    url::Url::parse(&with_slash).map(String::from).unwrap_or(with_slash)
}

/// `true` as soon as one signature validates against a trusted, unexpired
/// key over `name@version:integrity` — the trusted [`NPM_SIGNING_KEYS`] in
/// production, a test key in unit tests.
pub(super) fn signature_validates_against(
    component: &EngineComponent,
    signatures: &[PackageSignature],
    published_at: Option<&str>,
    keys: &[NpmSigningKey<'_>],
) -> bool {
    let message = format!("{}@{}:{}", component.name, component.version, component.integrity);
    let published_time = published_at.and_then(parse_timestamp);
    for signature in signatures {
        let Some(key) = keys.iter().find(|key| key.keyid == signature.keyid) else {
            continue;
        };
        let expired = match (key.expires.and_then(parse_timestamp), published_time) {
            (Some(expires), Some(published)) => published >= expires,
            _ => false,
        };
        if expired {
            continue;
        }
        if verify_one(key.key, &message, &signature.sig) {
            return true;
        }
    }
    false
}

/// Verify one base64 ECDSA-P256 signature over `message` against a base64
/// SPKI public key. Malformed key/signature bytes count as a non-match.
/// Same crypto core as `audit signatures`' [`verify_one`].
pub(super) fn verify_one(public_key_base64: &str, message: &str, signature_base64: &str) -> bool {
    let engine = base64::engine::general_purpose::STANDARD;
    let Ok(key_der) = engine.decode(public_key_base64) else {
        return false;
    };
    let Ok(verifying_key) = VerifyingKey::from_public_key_der(&key_der) else {
        return false;
    };
    let Ok(signature_der) = engine.decode(signature_base64) else {
        return false;
    };
    let Ok(signature) = Signature::from_der(&signature_der) else {
        return false;
    };
    verifying_key.verify(message.as_bytes(), &signature).is_ok()
}

fn parse_timestamp(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value).ok().map(|datetime| datetime.timestamp_millis())
}

#[derive(Deserialize)]
pub(super) struct PackageSignature {
    pub(super) keyid: String,
    pub(super) sig: String,
}

#[derive(Deserialize)]
pub(super) struct Dist {
    #[serde(default)]
    signatures: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub(super) struct PackumentVersion {
    #[serde(default)]
    pub(super) dist: Option<Dist>,
}

#[derive(Deserialize)]
struct Packument {
    #[serde(default)]
    time: std::collections::BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    versions: std::collections::HashMap<String, PackumentVersion>,
}

/// Fetch a component's packument from `registry` — its own (trusted)
/// registry, or the canonical fallback. `Ok(None)` for a 404 (package
/// absent); `Err` for any other failure (treated as `unreachable` by the
/// caller).
async fn fetch_packument(
    component: &EngineComponent,
    registry: &str,
    client: &ThrottledClient,
    retry_opts: RetryOpts,
    config: &Config,
) -> Result<Option<Packument>, String> {
    let packument_url =
        format!("{}{}", with_trailing_slash(registry), encode_package_name(&component.name));
    let display_url = redact_and_sanitize(&packument_url);
    // Resolve auth against the request URL *and* the package name so a
    // `@scope:registry`-scoped token applies (plain `for_url` skips the
    // scope lookup, breaking bootstrap registries that require it).
    let authorization = config
        .package_manager_bootstrap
        .auth_headers
        .for_url_with_package(&packument_url, Some(&component.name));

    let (_guard, response) = send_with_retry(client, &packument_url, retry_opts, |client| {
        let mut request = client.get(&packument_url).header("accept", "application/json");
        if let Some(value) = &authorization {
            request = request.header("authorization", value);
        }
        request
    })
    .await
    .map_err(|source| format!("{display_url}: {}", redact_and_sanitize(&source.to_string())))?;

    let status = response.status().as_u16();
    if status == 404 {
        return Ok(None);
    }
    if status != 200 {
        return Err(format!("{display_url} responded with {status}"));
    }
    let body_bytes = read_bounded_body(response, &display_url).await?;
    serde_json::from_slice::<Packument>(&body_bytes)
        .map(Some)
        .map_err(|err| format!("{display_url} returned invalid JSON: {err}"))
}

/// Bound the buffered body so an oversized response from a
/// misconfigured/compromised registry can't exhaust memory on this
/// trust-critical path.
async fn read_bounded_body(
    response: reqwest::Response,
    display_url: &str,
) -> Result<Vec<u8>, String> {
    if let Some(length) = response.content_length()
        && length > MAX_PACKUMENT_BYTES
    {
        return Err(format!("{display_url} returned an oversized packument ({length} bytes)"));
    }
    use futures_util::StreamExt as _;
    let mut stream = response.bytes_stream();
    let mut body_bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|source| {
            format!("{display_url}: {}", redact_and_sanitize(&source.to_string()))
        })?;
        if (body_bytes.len() + chunk.len()) as u64 > MAX_PACKUMENT_BYTES {
            return Err(format!("{display_url} returned an oversized packument"));
        }
        body_bytes.extend_from_slice(&chunk);
    }
    Ok(body_bytes)
}

/// Upper bound on a buffered packument response. Generous relative to the
/// pnpm / `@pnpm/exe` packuments (well under a megabyte) while still
/// capping a runaway response.
const MAX_PACKUMENT_BYTES: u64 = 50 * 1024 * 1024;

/// Route a (possibly scoped) engine component to its registry, using the
/// trusted package-manager bootstrap configuration.
pub(super) fn pick_registry(name: &str, config: &Config) -> String {
    let bootstrap = &config.package_manager_bootstrap;
    if let Some(scope) = name.strip_prefix('@').and_then(|rest| rest.split('/').next())
        && let Some(registry) = bootstrap.registries.get(&format!("@{scope}"))
    {
        return registry.clone();
    }
    bootstrap.registry.clone()
}

pub(super) fn build_client(config: &Config) -> Result<ThrottledClient, SelfUpdateError> {
    let bootstrap = &config.package_manager_bootstrap;
    ThrottledClient::for_installs(
        &bootstrap.proxy,
        &bootstrap.tls,
        &bootstrap.tls_by_uri,
        &config.network_settings(),
    )
    .map_err(|error| SelfUpdateError::EngineIdentityUnverifiable {
        message: format!("could not build the network client to verify the pnpm release: {error}"),
    })
}

fn with_trailing_slash(registry: &str) -> String {
    if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") }
}
