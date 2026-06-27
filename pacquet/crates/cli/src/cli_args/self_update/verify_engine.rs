//! Verify that the pnpm engine about to be installed and executed is the
//! genuinely-published `pnpm`.
//!
//! The wanted pnpm version comes from the resolved env lockfile, and the
//! project controls the lockfile integrity and the registry the bytes are
//! fetched from — so without this check a cloned repository could make
//! pnpm download and run an arbitrary native binary. The signed message
//! is built from the lockfile integrity and verified against npm's
//! embedded public keys (so a project-controlled registry cannot answer
//! with its own key pair); the signed packument is fetched from the
//! trusted package-manager bootstrap registry, which an npm mirror
//! proxies transparently.
//!
//! Runs only on a genuine download (a store cache miss), so it does not
//! add a network round trip to every command.

use crate::cli_args::package_name::encode_package_name;
use base64::Engine as _;
use p256::{
    ecdsa::{Signature, VerifyingKey, signature::Verifier},
    pkcs8::DecodePublicKey,
};
use pacquet_config::Config;
use pacquet_graph_hasher::{host_arch, host_libc, host_platform};
use pacquet_lockfile::{EnvLockfile, PackageKey, SnapshotDepRef};
use pacquet_network::{
    NetworkSettings, RetryOpts, ThrottledClient, redact_url_credentials, send_with_retry,
};
use serde::Deserialize;
use std::time::Duration;

use super::{
    SelfUpdateError,
    install_pnpm::{exe_platform_pkg_dir_name, exe_platform_pkg_dir_name_next},
};

/// npm's public registry signing keys, mirrored from
/// <https://registry.npmjs.org/-/npm/v1/keys>. `expires` is `None` for a
/// key with no expiry.
const NPM_SIGNING_KEYS: &[NpmSigningKey] = &[
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

struct NpmSigningKey<'a> {
    keyid: &'a str,
    key: &'a str,
    expires: Option<&'a str>,
}

/// A pnpm-engine component whose registry signature must validate over the
/// bytes the lockfile pins.
struct EngineComponent {
    name: String,
    registry: String,
    version: String,
    integrity: String,
}

/// Verify the pnpm engine recorded in `env` against npm's embedded keys.
///
/// Returns an error when verification detects tampering (an invalid
/// signature), when a component is absent from the registry, when a
/// component carries no integrity metadata, or when the registry is
/// unreachable — failing closed in every case, since the lockfile
/// integrity is project-controlled and not a safe fallback.
pub(crate) async fn verify_pnpm_engine_identity(
    env: &EnvLockfile,
    pnpm_version: &str,
    config: &Config,
) -> Result<(), SelfUpdateError> {
    let to_verify = collect_engine_components(env, config)?;
    if to_verify.is_empty() {
        return Err(SelfUpdateError::EngineIdentityUnverifiable {
            message: format!(
                "Cannot verify the identity of pnpm@{pnpm_version}: its integrity metadata is missing from pnpm-lock.yaml.",
            ),
        });
    }

    let client = build_client(config)?;
    let retry_opts = retry_opts(config);

    let mut failures: Vec<SignatureFailure> = Vec::new();
    for component in &to_verify {
        if let Some(failure) = find_signature_failure(component, &client, retry_opts, config).await
        {
            failures.push(failure);
        }
    }
    if failures.is_empty() {
        return Ok(());
    }
    failures.sort_by(|left, right| left.label.cmp(&right.label));

    let only_unreachable =
        failures.iter().all(|failure| failure.category == FailureCategory::Unreachable);
    let described = failures.iter().map(SignatureFailure::describe).collect::<Vec<_>>().join("; ");
    let message = format!(
        "Refusing to run pnpm@{pnpm_version}: its npm registry signature could not be verified \
         ({described}). The bytes selected by this project's lockfile/registry do not match a \
         published, signed pnpm release.",
    );
    if only_unreachable {
        Err(SelfUpdateError::EngineIdentityUnverifiable { message })
    } else {
        Err(SelfUpdateError::EngineIdentityMismatch { message })
    }
}

/// Collect the engine components to verify from the env lockfile: `pnpm`,
/// `@pnpm/exe`, and the host's `@pnpm/exe` platform binary (an optional
/// dependency of `@pnpm/exe`). Errors if a present component carries no
/// integrity.
fn collect_engine_components(
    env: &EnvLockfile,
    config: &Config,
) -> Result<Vec<EngineComponent>, SelfUpdateError> {
    let mut to_verify = Vec::new();
    let pm_deps = env
        .importers
        .get(EnvLockfile::ROOT_IMPORTER_KEY)
        .and_then(|importer| importer.package_manager_dependencies.as_ref());
    let Some(pm_deps) = pm_deps else {
        return Ok(to_verify);
    };

    for name in ["pnpm", "@pnpm/exe"] {
        if let Some(dep) = pm_deps.get(name) {
            to_verify.push(engine_component(env, config, name, &dep.version)?);
        }
    }

    // The bytes actually executed are the host's `@pnpm/exe` platform
    // binary, listed as an optional dependency of `@pnpm/exe`. Since this
    // is the native code self-update will run, a missing snapshot, missing
    // optional deps, or no host candidate fails closed rather than letting
    // verification pass on `pnpm`/`@pnpm/exe` alone.
    if let Some(exe) = pm_deps.get("@pnpm/exe") {
        let exe_version = &exe.version;
        let snapshot_label = format!("@pnpm/exe@{exe_version}");
        let snapshot_key = snapshot_label.parse::<PackageKey>().map_err(|_| {
            SelfUpdateError::EngineIdentityUnverifiable {
                message: format!(
                    "Cannot verify the identity of {snapshot_label}: its lockfile snapshot key is invalid.",
                ),
            }
        })?;
        let optional_deps = env
            .snapshots
            .get(&snapshot_key)
            .and_then(|snapshot| snapshot.optional_dependencies.as_ref())
            .ok_or_else(|| SelfUpdateError::EngineIdentityUnverifiable {
                message: format!(
                    "Cannot verify the identity of {snapshot_label}: its platform binaries are missing from pnpm-lock.yaml.",
                ),
            })?;
        let platform = host_platform();
        let arch = host_arch();
        let libc = host_libc();
        let candidate_names = [
            format!("@pnpm/{}", exe_platform_pkg_dir_name(platform, arch, libc)),
            format!("@pnpm/{}", exe_platform_pkg_dir_name_next(platform, arch, libc)),
        ];
        let platform_dep = candidate_names.iter().find_map(|platform_name| {
            let key = platform_name.parse().ok()?;
            let version = plain_version(optional_deps.get(&key)?)?;
            Some((platform_name.clone(), version))
        });
        // The first candidate present in the lockfile is the binary the
        // install links and executes.
        let Some((platform_name, version)) = platform_dep else {
            return Err(SelfUpdateError::EngineIdentityUnverifiable {
                message: format!(
                    "Cannot verify the identity of the @pnpm/exe.{platform}-{arch} native binary: it is missing from pnpm-lock.yaml.",
                ),
            });
        };
        to_verify.push(engine_component(env, config, &platform_name, &version)?);
    }

    Ok(to_verify)
}

/// Build the [`EngineComponent`] for `name@version`, reading its integrity
/// from the env lockfile's `packages:` map. A missing integrity fails
/// closed.
fn engine_component(
    env: &EnvLockfile,
    config: &Config,
    name: &str,
    version: &str,
) -> Result<EngineComponent, SelfUpdateError> {
    let integrity = format!("{name}@{version}")
        .parse::<PackageKey>()
        .ok()
        .and_then(|key| env.packages.get(&key).map(|metadata| metadata.resolution.integrity()))
        .flatten()
        .map(ToString::to_string);
    let Some(integrity) = integrity.filter(|integrity| !integrity.is_empty()) else {
        return Err(SelfUpdateError::EngineIdentityUnverifiable {
            message: format!(
                "Cannot verify the identity of {name}@{version}: its integrity metadata is missing from pnpm-lock.yaml.",
            ),
        });
    };
    Ok(EngineComponent {
        name: name.to_string(),
        registry: pick_registry(name, config),
        version: version.to_string(),
        integrity,
    })
}

/// The exact version of a plain (non-alias, non-link) snapshot reference.
fn plain_version(reference: &SnapshotDepRef) -> Option<String> {
    match reference {
        SnapshotDepRef::Plain(ver_peer) => {
            // Strip any peer suffix; an `@pnpm/exe` platform optional dep
            // is always an exact, peerless version.
            Some(ver_peer.to_string().split('(').next().unwrap_or_default().to_string())
        }
        SnapshotDepRef::Alias(_) | SnapshotDepRef::Link(_) => None,
    }
}

#[derive(PartialEq, Eq)]
enum FailureCategory {
    Invalid,
    Absent,
    Unreachable,
}

struct SignatureFailure {
    label: String,
    reason: String,
    category: FailureCategory,
}

impl SignatureFailure {
    fn describe(&self) -> String {
        format!("{}: {}", self.label, self.reason)
    }
}

/// Per-component verification. Returns `None` when a registry signature
/// validates over the lockfile bytes.
async fn find_signature_failure(
    component: &EngineComponent,
    client: &ThrottledClient,
    retry_opts: RetryOpts,
    config: &Config,
) -> Option<SignatureFailure> {
    let label = format!("{}@{}", component.name, component.version);
    let packument = match fetch_packument(component, client, retry_opts, config).await {
        Ok(Some(packument)) => packument,
        Ok(None) => {
            return Some(SignatureFailure {
                reason: format!("{} is not published on {}", component.name, component.registry),
                category: FailureCategory::Absent,
                label,
            });
        }
        Err(reason) => {
            return Some(SignatureFailure {
                reason,
                category: FailureCategory::Unreachable,
                label,
            });
        }
    };

    let Some(version) = packument.versions.get(&component.version) else {
        return Some(SignatureFailure {
            reason: format!("{label} was not found on {}", component.registry),
            category: FailureCategory::Absent,
            label,
        });
    };
    let raw_signatures = version.dist.as_ref().and_then(|dist| dist.signatures.as_ref());
    let parsed_signatures = match raw_signatures {
        None => Vec::new(),
        Some(serde_json::Value::Array(elements)) => {
            let mut parsed = Vec::with_capacity(elements.len());
            for element in elements {
                let Ok(signature) = serde_json::from_value::<PackageSignature>(element.clone())
                else {
                    return Some(SignatureFailure {
                        reason: format!("malformed registry signatures metadata for {label}"),
                        category: FailureCategory::Absent,
                        label,
                    });
                };
                parsed.push(signature);
            }
            parsed
        }
        Some(_) => {
            return Some(SignatureFailure {
                reason: format!("malformed registry signatures metadata for {label}"),
                category: FailureCategory::Absent,
                label,
            });
        }
    };
    if parsed_signatures.is_empty() {
        return Some(SignatureFailure {
            reason: format!("{label} has no registry signature"),
            category: FailureCategory::Absent,
            label,
        });
    }

    let published_at = packument.time.get(&component.version).and_then(serde_json::Value::as_str);
    // The message is built from the *lockfile* integrity, so a signature
    // only validates when the installed bytes match what the registry
    // signed.
    if signature_validates(component, &parsed_signatures, published_at) {
        None
    } else {
        Some(SignatureFailure {
            reason: "invalid registry signature".to_string(),
            category: FailureCategory::Invalid,
            label,
        })
    }
}

/// `true` as soon as one signature validates against a trusted, unexpired
/// npm key over `name@version:integrity`.
fn signature_validates(
    component: &EngineComponent,
    signatures: &[PackageSignature],
    published_at: Option<&str>,
) -> bool {
    signature_validates_against(component, signatures, published_at, NPM_SIGNING_KEYS)
}

/// [`signature_validates`] against an explicit key set — the trusted
/// [`NPM_SIGNING_KEYS`] in production, a test key in unit tests.
fn signature_validates_against(
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
fn verify_one(public_key_base64: &str, message: &str, signature_base64: &str) -> bool {
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
struct PackageSignature {
    keyid: String,
    sig: String,
}

#[derive(Deserialize)]
struct Dist {
    #[serde(default)]
    signatures: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct PackumentVersion {
    #[serde(default)]
    dist: Option<Dist>,
}

#[derive(Deserialize)]
struct Packument {
    #[serde(default)]
    time: std::collections::BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    versions: std::collections::HashMap<String, PackumentVersion>,
}

/// Fetch a component's packument from its (trusted) registry. `Ok(None)`
/// for a 404 (package absent); `Err` for any other failure (treated as
/// `unreachable` by the caller, so verification fails closed).
async fn fetch_packument(
    component: &EngineComponent,
    client: &ThrottledClient,
    retry_opts: RetryOpts,
    config: &Config,
) -> Result<Option<Packument>, String> {
    let registry_url = with_trailing_slash(&component.registry);
    let packument_url = format!("{registry_url}{}", encode_package_name(&component.name));
    let display_url = redact_url_credentials(&packument_url);
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
    .map_err(|source| format!("{display_url}: {}", redact_url_credentials(&source.to_string())))?;

    let status = response.status().as_u16();
    if status == 404 {
        return Ok(None);
    }
    if status != 200 {
        return Err(format!("{display_url} responded with {status}"));
    }
    // Bound the buffered body so an oversized response from a
    // misconfigured/compromised registry can't exhaust memory on this
    // trust-critical path.
    if let Some(length) = response.content_length()
        && length > MAX_PACKUMENT_BYTES
    {
        return Err(format!("{display_url} returned an oversized packument ({length} bytes)"));
    }
    let body = response.text().await.map_err(|source| {
        format!("{display_url}: {}", redact_url_credentials(&source.to_string()))
    })?;
    if body.len() as u64 > MAX_PACKUMENT_BYTES {
        return Err(format!("{display_url} returned an oversized packument"));
    }
    serde_json::from_str::<Packument>(&body)
        .map(Some)
        .map_err(|err| format!("{display_url} returned invalid JSON: {err}"))
}

/// Upper bound on a buffered packument response. Generous relative to the
/// pnpm / `@pnpm/exe` packuments (well under a megabyte) while still
/// capping a runaway response.
const MAX_PACKUMENT_BYTES: u64 = 50 * 1024 * 1024;

/// Route a (possibly scoped) engine component to its registry, using the
/// trusted package-manager bootstrap configuration.
fn pick_registry(name: &str, config: &Config) -> String {
    let bootstrap = &config.package_manager_bootstrap;
    if let Some(scope) = name.strip_prefix('@').and_then(|rest| rest.split('/').next())
        && let Some(registry) = bootstrap.registries.get(&format!("@{scope}"))
    {
        return registry.clone();
    }
    bootstrap.registry.clone()
}

fn build_client(config: &Config) -> Result<ThrottledClient, SelfUpdateError> {
    let bootstrap = &config.package_manager_bootstrap;
    ThrottledClient::for_installs(
        &bootstrap.proxy,
        &bootstrap.tls,
        &bootstrap.tls_by_uri,
        &NetworkSettings {
            network_concurrency: config.network_concurrency,
            fetch_timeout: Duration::from_millis(config.fetch_timeout),
            user_agent: config.user_agent.clone(),
        },
    )
    .map_err(|error| SelfUpdateError::EngineIdentityUnverifiable {
        message: format!("could not build the network client to verify the pnpm release: {error}"),
    })
}

fn retry_opts(config: &Config) -> RetryOpts {
    RetryOpts {
        retries: config.fetch_retries,
        factor: config.fetch_retry_factor,
        min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
        max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
    }
}

fn with_trailing_slash(registry: &str) -> String {
    if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") }
}

#[cfg(test)]
mod tests;
