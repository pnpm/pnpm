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

use base64::Engine as _;
use p256::{
    ecdsa::{Signature, VerifyingKey, signature::Verifier},
    pkcs8::DecodePublicKey,
};
use pnpm_config::Config;
use pnpm_graph_hasher::{host_arch, host_libc, host_platform};
use pnpm_lockfile::{EnvLockfile, PackageKey, SnapshotDepRef, SpecifierAndResolution};
use pnpm_network::{
    RetryOpts, ThrottledClient, encode_package_name, redact_and_sanitize, send_with_retry,
};
use serde::Deserialize;
use std::{collections::BTreeMap, time::Duration};

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

/// The canonical npm registry, consulted for signature metadata when a
/// user-configured registry cannot provide a verifiable signature. Where the
/// signature bytes come from does not affect what they prove: they are
/// verified against the embedded keys over the lockfile integrity, so a
/// component passes only when a genuine signature validates over the bytes
/// actually installed. See <https://github.com/pnpm/pnpm/issues/13147>.
const CANONICAL_NPM_REGISTRY: &str = "https://registry.npmjs.org/";

/// A package-manager engine component whose registry signature must
/// validate over the bytes the lockfile pins.
struct EngineComponent {
    name: String,
    registry: String,
    version: String,
    integrity: String,
}

/// The engine whose identity is being checked.
pub(crate) struct EngineToVerify<'a> {
    /// `<name>@<version>` as the user asked for it, for diagnostics.
    pub(crate) label: &'a str,
    /// The packages the env lockfile pins for the engine.
    pub(crate) packages: &'a [&'a str],
    pub(crate) platform_binaries: PlatformBinaries,
}

/// How an engine ships the native code that actually executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlatformBinaries {
    /// `@pnpm/exe.<target>` packages, listed as optional dependencies of
    /// the pnpm wrapper.
    PnpmExe,
    /// The engine is a JavaScript CLI — the pinned packages are all there
    /// is to verify.
    None,
}

/// Verify the package-manager engine recorded in `env` against npm's
/// embedded keys.
///
/// Registries that serve no `dist.signatures` (private mirrors and feed
/// proxies commonly strip them) do not fail the check outright: the
/// signature is fetched from [`CANONICAL_NPM_REGISTRY`] instead, which
/// proves exactly the same thing. When no signature can be obtained from
/// either source (both unreachable, or the integrity is a non-sha512 pin no
/// npm signature can cover), the check returns a warning for the caller to
/// emit and lets the install proceed — but only when every engine component
/// resolves through a non-canonical registry. Such a registry can only come
/// from the user's own trusted (non-project) configuration, the download URL
/// is derived from it rather than read from the lockfile, and the bytes stay
/// pinned by the lockfile integrity — so a cloned repository still cannot
/// steer pnpm to attacker-controlled bytes; the residual trust is the same
/// the user already places in that registry for every package installed
/// from it.
///
/// Returns `Ok(None)` when a signature validated, `Ok(Some(warning))` when
/// the install may proceed unverified, and an error when verification
/// detects tampering (an invalid signature), when a component is absent from
/// a reachable canonical registry, when a component carries no integrity
/// metadata, or when the canonical registry is the configured registry and
/// is unreachable — the lockfile integrity is project-controlled and not a
/// safe fallback there.
pub(crate) async fn verify_engine_identity(
    env: &EnvLockfile,
    engine: &EngineToVerify<'_>,
    config: &Config,
) -> Result<Option<String>, SelfUpdateError> {
    let label = engine.label;
    let to_verify = collect_engine_components(env, config, engine)?;
    if to_verify.is_empty() {
        return Err(SelfUpdateError::EngineIdentityUnverifiable {
            message: format!(
                "Cannot verify the identity of {label}: its integrity metadata is missing from pnpm-lock.yaml.",
            ),
        });
    }

    let client = build_client(config)?;
    let retry_opts = retry_opts(config);

    let mut failures: Vec<SignatureFailure> = Vec::new();
    for component in &to_verify {
        if let Some(failure) = find_signature_failure(
            component,
            CANONICAL_NPM_REGISTRY,
            NPM_SIGNING_KEYS,
            &client,
            retry_opts,
            config,
        )
        .await
        {
            failures.push(failure);
        }
    }
    if failures.is_empty() {
        return Ok(None);
    }
    failures.sort_by(|left, right| left.label.cmp(&right.label));
    let described = failures.iter().map(SignatureFailure::describe).collect::<Vec<_>>().join("; ");

    if failures.iter().all(SignatureFailure::tolerable_without_signature) {
        return Ok(Some(format!(
            "The authenticity of {label} could not be verified against npm's registry \
             signatures: {described}. Proceeding anyway, because the release was resolved through \
             the registry configured in your own (non-project) configuration and stays pinned by \
             its integrity checksum.",
        )));
    }

    let only_unreachable =
        failures.iter().all(|failure| failure.category == FailureCategory::Unreachable);
    let message = format!(
        "Refusing to run {label}: its npm registry signature could not be verified \
         ({described}). The bytes its environment lockfile pins, resolved through the configured \
         package-manager registry, do not match a published, signed release.",
    );
    if only_unreachable {
        Err(SelfUpdateError::EngineIdentityUnverifiable { message })
    } else {
        Err(SelfUpdateError::EngineIdentityMismatch { message })
    }
}

/// Collect the engine components to verify from the env lockfile: the
/// engine's own packages, plus — for pnpm — the host's platform binary (an
/// optional dependency of the native wrapper, see [`native_engine_wrapper`]).
/// Errors if a present component carries no integrity.
fn collect_engine_components(
    env: &EnvLockfile,
    config: &Config,
    engine: &EngineToVerify<'_>,
) -> Result<Vec<EngineComponent>, SelfUpdateError> {
    let mut to_verify = Vec::new();
    let pm_deps = env
        .importers
        .get(EnvLockfile::ROOT_IMPORTER_KEY)
        .and_then(|importer| importer.package_manager_dependencies.as_ref());
    let Some(pm_deps) = pm_deps else {
        return Ok(to_verify);
    };

    for name in engine.packages {
        // The engine's package list is derived from the version being
        // installed, so a package missing from the lockfile that pins it
        // means the two disagree about what is about to run.
        let dep =
            pm_deps.get(*name).ok_or_else(|| SelfUpdateError::EngineIdentityUnverifiable {
                message: format!(
                    "Cannot verify the identity of {}: {name} is missing from pnpm-lock.yaml.",
                    engine.label,
                ),
            })?;
        to_verify.push(engine_component(env, config, name, &dep.version)?);
    }

    if engine.platform_binaries == PlatformBinaries::None {
        return Ok(to_verify);
    }

    // The bytes actually executed are the host's platform binary, listed as
    // an optional dependency of the native wrapper. Since this is the native
    // code that will run, a missing snapshot, missing optional deps, or no
    // host candidate fails closed rather than letting verification pass on
    // the wrappers alone.
    if let Some((wrapper_name, wrapper_version)) = native_engine_wrapper(pm_deps) {
        let snapshot_label = format!("{wrapper_name}@{wrapper_version}");
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

/// The engine package whose optional dependencies carry the host's native
/// binary: `@pnpm/exe` when the lockfile pins it, otherwise `pnpm` itself
/// for `>=12`, where the unscoped package is the native executable. `None`
/// when the lockfile pins only a JS-only `pnpm` (`<6.17.1`), which has no
/// platform binaries.
fn native_engine_wrapper(
    pm_deps: &BTreeMap<String, SpecifierAndResolution>,
) -> Option<(&str, &str)> {
    if let Some(exe) = pm_deps.get("@pnpm/exe") {
        return Some(("@pnpm/exe", &exe.version));
    }
    let pnpm = pm_deps.get("pnpm")?;
    let version = node_semver::Version::parse(&pnpm.version).ok()?;
    (version.major >= 12).then_some(("pnpm", pnpm.version.as_str()))
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
    /// The lockfile integrity is not a sha512 hash (e.g. a sha1 pin from a
    /// registry that publishes only `shasum`), so no npm registry signature
    /// can ever validate over it — verification is impossible by
    /// construction, not evidence of tampering.
    Uncovered,
}

struct SignatureFailure {
    label: String,
    registry: String,
    reason: String,
    category: FailureCategory,
}

impl SignatureFailure {
    fn describe(&self) -> String {
        format!("{}: {}", self.label, self.reason)
    }

    /// Whether the engine may run despite this failure: no signature was
    /// obtainable (nothing suspicious was observed — as opposed to a
    /// signature that exists but does not validate, or the canonical
    /// registry answering that no signed release exists), and the component
    /// resolves through a registry the user configured themselves.
    fn tolerable_without_signature(&self) -> bool {
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
async fn find_signature_failure(
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

    // A well-formed signature that fails to validate is a tamper signal from
    // either source; surface it over the softer categories.
    if primary.1 == FailureCategory::Invalid {
        return failure(primary.0, primary.1);
    }
    if secondary.1 != FailureCategory::Unreachable {
        return failure(secondary.0, secondary.1);
    }
    // The primary registry had no usable signature (a mirror commonly serves
    // none) and the fallback could not be consulted — nothing suspicious was
    // observed, the signature was simply unobtainable.
    failure(
        format!(
            "{}; the fallback registry ({}) could not be consulted either: {}",
            primary.0,
            redact_and_sanitize(fallback_registry),
            secondary.0,
        ),
        FailureCategory::Unreachable,
    )
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
    let parsed_signatures = match raw_signatures {
        None => Vec::new(),
        Some(serde_json::Value::Array(elements)) => {
            let mut parsed = Vec::with_capacity(elements.len());
            for element in elements {
                let Ok(signature) = serde_json::from_value::<PackageSignature>(element.clone())
                else {
                    return Some((
                        format!("malformed registry signatures metadata for {label}"),
                        FailureCategory::Absent,
                    ));
                };
                parsed.push(signature);
            }
            parsed
        }
        Some(_) => {
            return Some((
                format!("malformed registry signatures metadata for {label}"),
                FailureCategory::Absent,
            ));
        }
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
    let registry_url = with_trailing_slash(registry);
    let packument_url = format!("{registry_url}{}", encode_package_name(&component.name));
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
    // Bound the buffered body so an oversized response from a
    // misconfigured/compromised registry can't exhaust memory on this
    // trust-critical path.
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
    serde_json::from_slice::<Packument>(&body_bytes)
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
        &config.network_settings(),
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
