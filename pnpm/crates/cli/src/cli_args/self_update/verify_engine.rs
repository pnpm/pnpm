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

use super::{
    SelfUpdateError,
    install_pnpm::{exe_platform_pkg_dir_name, exe_platform_pkg_dir_name_next, native_target_name},
};
use p256::ecdsa::{Signature, VerifyingKey};
use pnpm_config::Config;
use pnpm_graph_hasher::{host_arch, host_libc, host_platform};
use pnpm_lockfile::{EnvLockfile, PackageKey, PkgName, SnapshotDepRef};
use pnpm_network::{
    RetryOpts, ThrottledClient, encode_package_name, redact_and_sanitize, send_with_retry,
};
use serde::Deserialize;
use signatures::{
    CANONICAL_NPM_REGISTRY, FailureCategory, NPM_SIGNING_KEYS, SignatureFailure, build_client,
    find_signature_failure, pick_registry,
};
use std::collections::HashMap;

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
    /// The package the install is rooted at and then runs. The env lockfile
    /// pins the JavaScript `pnpm` and the SEA `@pnpm/exe` side by side below
    /// v12, but an install materializes only one of them, so a package that
    /// ships no binary for the host must not block running the other.
    pub(crate) package: &'a str,
    /// The exact version of [`Self::package`] the install materializes.
    pub(crate) version: &'a str,
    pub(crate) platform_binaries: PlatformBinaries,
}

impl EngineToVerify<'_> {
    /// `<name>@<version>` of the package that is actually installed, which
    /// is `@pnpm/exe@…` where [`Self::label`] says `pnpm@…`.
    fn package_label(&self) -> String {
        format!("{}@{}", self.package, self.version)
    }
}

/// How an engine ships the native code that actually executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlatformBinaries {
    /// `@pnpm/exe.<target>` packages, listed as optional dependencies of
    /// the pnpm wrapper.
    PnpmExe,
    /// The engine is a JavaScript CLI, so the package itself is all there
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

    let client = build_client(config)?;
    let retry_opts = config.retry_opts();

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
    report_identity_failures(label, failures)
}

/// Collect the engine components to verify from the env lockfile: the one
/// package the install is rooted at, plus the host's platform package among
/// its optional dependencies. Errors if a component carries no integrity, or
/// if a native engine has no platform package for the host.
fn collect_engine_components(
    env: &EnvLockfile,
    config: &Config,
    engine: &EngineToVerify<'_>,
) -> Result<Vec<EngineComponent>, SelfUpdateError> {
    let package_label = engine.package_label();
    verify_engine_pin(env, engine, &package_label)?;
    let mut to_verify = vec![engine_component(env, config, engine.package, engine.version)?];

    // `link_exe_platform_binary` hardlinks the host's platform binary over
    // the engine's own `pnpm` bin, so whenever the lockfile carries a
    // candidate for this host those are the bytes that execute and they must
    // be verified — whichever engine lists them.
    let snapshot_key = package_label.parse::<PackageKey>().map_err(|_| {
        SelfUpdateError::EngineIdentityUnverifiable {
            message: format!(
                "Cannot verify the identity of {package_label}: its lockfile snapshot key is invalid.",
            ),
        }
    })?;
    let optional_deps = env
        .snapshots
        .get(&snapshot_key)
        .and_then(|snapshot| snapshot.optional_dependencies.as_ref());
    if let Some((platform_name, version)) = optional_deps.and_then(host_platform_package) {
        to_verify.push(engine_component(env, config, &platform_name, &version)?);
        return Ok(to_verify);
    }
    // A JavaScript engine runs on Node.js and has no binary to be missing.
    if engine.platform_binaries == PlatformBinaries::None {
        return Ok(to_verify);
    }

    // The native code that will run is unaccounted for, so this fails closed
    // rather than letting verification pass on the wrapper alone.
    if optional_deps.is_none() {
        return Err(SelfUpdateError::EngineIdentityUnverifiable {
            message: format!(
                "Cannot verify the identity of {package_label}: its platform binaries are missing from pnpm-lock.yaml.",
            ),
        });
    }
    Err(SelfUpdateError::EngineNoNativeBinary {
        label: package_label,
        target: native_target_name(host_platform(), host_arch(), host_libc()),
    })
}

/// The platform package among an engine's `optional_deps` that the install
/// links and executes on this host: the first of the legacy
/// `@pnpm/<os>-<arch>` and the `@pnpm/exe.<target>` names present, the same
/// order `link_exe_platform_binary` searches. `None` when the engine ships
/// no binary for the host.
fn host_platform_package(
    optional_deps: &HashMap<PkgName, SnapshotDepRef>,
) -> Option<(String, String)> {
    let platform = host_platform();
    let arch = host_arch();
    let libc = host_libc();
    let candidate_names = [
        format!("@pnpm/{}", exe_platform_pkg_dir_name(platform, arch, libc)),
        format!("@pnpm/{}", exe_platform_pkg_dir_name_next(platform, arch, libc)),
    ];
    candidate_names.iter().find_map(|platform_name| {
        let key = platform_name.parse().ok()?;
        let version = plain_version(optional_deps.get(&key)?)?;
        Some((platform_name.clone(), version))
    })
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

#[cfg(test)]
mod tests;

fn report_identity_failures(
    label: &str,
    mut failures: Vec<SignatureFailure>,
) -> Result<Option<String>, SelfUpdateError> {
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

/// Verify precisely the root engine version whose bytes will execute.
fn verify_engine_pin(
    env: &EnvLockfile,
    engine: &EngineToVerify<'_>,
    package_label: &str,
) -> Result<(), SelfUpdateError> {
    let pinned = env
        .importers
        .get(EnvLockfile::ROOT_IMPORTER_KEY)
        .and_then(|importer| importer.package_manager_dependencies.as_ref())
        .and_then(|pm_deps| pm_deps.get(engine.package));
    if pinned.is_none_or(|dep| dep.version != engine.version) {
        return Err(SelfUpdateError::EngineIdentityUnverifiable {
            message: format!(
                "Cannot verify the identity of {package_label}: the environment lockfile does not pin it.",
            ),
        });
    }
    Ok(())
}

/// Invalid signatures take precedence over missing evidence from either registry.
fn classify_signature_failures(
    primary: (String, FailureCategory),
    secondary: (String, FailureCategory),
    fallback_registry: &str,
    failure: impl Fn(String, FailureCategory) -> Option<SignatureFailure>,
) -> Option<SignatureFailure> {
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

mod signatures;
