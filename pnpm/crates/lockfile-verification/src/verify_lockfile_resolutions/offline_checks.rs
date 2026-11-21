use std::{
    collections::BTreeSet,
    sync::Arc,
};

use pnpm_lockfile::{
    Lockfile,
    LockfileResolution,
    PkgName,
    is_git_hosted_tarball_url,
};
use pnpm_package_name::is_valid_old_npm_package_name;
use pnpm_resolving_resolver_base::{
    ResolutionPolicyViolation,
    ResolutionVerification,
    ResolutionVerifier,
    VerifyCtx,
    VerifyFuture,
};

use super::{
    ExoticSubdepsPolicy,
    ExoticSubdepsScope,
    VerifyLockfileResolutionsOptions,
    file_url_shape::FileShapeContext,
};
use crate::errors::VerifyError;

pub const RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE: &str = "RESOLUTION_SHAPE_MISMATCH";
pub const LOCAL_SUBDEP_NOT_ALLOWED_VIOLATION_CODE: &str = "LOCAL_SUBDEP_NOT_ALLOWED";

pub(super) fn verify_structure_before_install(
    lockfile: &Lockfile,
    opts: &VerifyLockfileResolutionsOptions<'_>,
) -> Result<(), VerifyError> {
    verify_lockfile_dependency_names(lockfile)?;
    let mut policy = opts.local_policy.clone();
    if let Some(lockfile_dir) = opts.lockfile_path.and_then(std::path::Path::parent) {
        let entry = policy.get_or_insert_with(Default::default);
        if entry.scope.lockfile_dir.is_none() {
            entry.scope.lockfile_dir = Some(Arc::from(lockfile_dir));
        }
    }
    super::local_containment::verify_lockfile_local_resolutions(lockfile, policy)?;
    if opts.repairs_lockfile {
        return Ok(());
    }
    verify_lockfile_importer_snapshot_links(lockfile)
}

/// Schema version recorded under `resolutionShapeCheck` in whole-lockfile
/// verification caches (`lockfile-verified.jsonl` and `pnpr` verdict caches).
/// Records carrying any other value (such as boolean `true` or a different
/// integer) miss `can_trust_past_check` and trigger re-verification.
pub const RESOLUTION_SHAPE_CACHE_VERSION: u64 = 2;

/// Cache-key participant for an always-on offline structural check
/// (resolution-shape, dependency-alias): a record lacking `flag: expected`
/// fails `can_trust_past_check` and forces re-verification. `verify` is never
/// invoked, the identity is appended only to the verifier lists handed to the
/// cache lookup and recorder.
struct OfflineCheckCacheIdentity {
    policy: serde_json::Map<String, serde_json::Value>,
    flag: &'static str,
    expected: serde_json::Value,
}

fn offline_cache_identity(
    flag: &'static str,
    expected: serde_json::Value,
) -> Arc<dyn ResolutionVerifier> {
    let mut policy = serde_json::Map::new();
    policy.insert(flag.to_string(), expected.clone());
    Arc::new(OfflineCheckCacheIdentity { policy, flag, expected })
}

fn resolution_shape_cache_identity() -> Arc<dyn ResolutionVerifier> {
    offline_cache_identity(
        "resolutionShapeCheck",
        serde_json::Value::from(RESOLUTION_SHAPE_CACHE_VERSION),
    )
}

fn dependency_alias_cache_identity() -> Arc<dyn ResolutionVerifier> {
    offline_cache_identity("dependencyAliasCheck", serde_json::Value::Bool(true))
}

fn variation_resolution_cache_identity() -> Arc<dyn ResolutionVerifier> {
    offline_cache_identity("variationResolutionCheck", serde_json::Value::Bool(true))
}

struct ExoticSubdepsCacheIdentity {
    policy: serde_json::Map<String, serde_json::Value>,
    scope_digest: String,
}

fn exotic_subdeps_cache_identity(scope: &ExoticSubdepsScope) -> Arc<dyn ResolutionVerifier> {
    let scope_digest = scope.cache_digest();
    let mut policy = serde_json::Map::new();
    policy.insert("blockExoticSubdeps".to_string(), serde_json::Value::Bool(true));
    policy.insert(
        "blockExoticSubdepsScope".to_string(),
        serde_json::Value::String(scope_digest.clone()),
    );
    Arc::new(ExoticSubdepsCacheIdentity { policy, scope_digest })
}

fn local_subdep_cache_identity() -> Arc<dyn ResolutionVerifier> {
    offline_cache_identity("localSubdepCheck", serde_json::Value::Bool(true))
}

/// Append the always-on offline structural checks' identities to `verifiers`
/// before consulting or recording a whole-lockfile verification cache, so a
/// record written before one of those rules existed fails
/// `can_trust_past_check` and forces re-verification.
#[must_use]
pub fn with_offline_check_cache_identities(
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> Vec<Arc<dyn ResolutionVerifier>> {
    with_offline_and_exotic_cache_identities(verifiers, &ExoticSubdepsPolicy::Allow)
}

pub(crate) fn with_offline_and_exotic_cache_identities(
    verifiers: &[Arc<dyn ResolutionVerifier>],
    exotic_subdeps: &ExoticSubdepsPolicy,
) -> Vec<Arc<dyn ResolutionVerifier>> {
    let exotic_identity = exotic_subdeps.scope().map(exotic_subdeps_cache_identity);
    verifiers
        .iter()
        .cloned()
        .chain([
            resolution_shape_cache_identity(),
            dependency_alias_cache_identity(),
            variation_resolution_cache_identity(),
            local_subdep_cache_identity(),
        ])
        .chain(exotic_identity)
        .collect()
}

/// Snapshot the combined policy map for all active verifiers and offline checks.
#[must_use]
pub fn snapshot_verification_policy(
    verifiers: &[Arc<dyn ResolutionVerifier>],
    exotic_subdeps: &ExoticSubdepsPolicy,
) -> serde_json::Map<String, serde_json::Value> {
    let mut merged = serde_json::Map::new();
    for verifier in with_offline_and_exotic_cache_identities(verifiers, exotic_subdeps) {
        for (key, value) in verifier.policy() {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged
}

/// Whether `recorded` satisfies every currently active verifier and offline check.
#[must_use]
pub fn can_trust_prepared_policy(
    recorded: &serde_json::Map<String, serde_json::Value>,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    exotic_subdeps: &ExoticSubdepsPolicy,
) -> bool {
    with_offline_and_exotic_cache_identities(verifiers, exotic_subdeps)
        .iter()
        .all(|verifier| verifier.can_trust_past_check(recorded))
}

impl ResolutionVerifier for OfflineCheckCacheIdentity {
    fn verify<'a>(
        &'a self,
        _resolution: &'a LockfileResolution,
        _ctx: VerifyCtx<'a>,
    ) -> VerifyFuture<'a> {
        Box::pin(async { ResolutionVerification::Ok })
    }

    fn policy(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.policy
    }

    fn can_trust_past_check(&self, cached: &serde_json::Map<String, serde_json::Value>) -> bool {
        cached.get(self.flag) == Some(&self.expected)
    }
}

impl ResolutionVerifier for ExoticSubdepsCacheIdentity {
    fn verify<'a>(
        &'a self,
        _resolution: &'a LockfileResolution,
        _ctx: VerifyCtx<'a>,
    ) -> VerifyFuture<'a> {
        Box::pin(async { ResolutionVerification::Ok })
    }

    fn policy(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.policy
    }

    fn can_trust_past_check(&self, cached: &serde_json::Map<String, serde_json::Value>) -> bool {
        cached.get("blockExoticSubdeps") == Some(&serde_json::Value::Bool(true))
            && cached.get("blockExoticSubdepsScope").and_then(serde_json::Value::as_str)
                == Some(self.scope_digest.as_str())
    }
}

/// Whether a resolution is registry-shaped. A plain tarball
/// resolution is registry-shaped because the npm verifier unconditionally
/// binds explicit tarball URLs of semver-keyed entries to the registry's
/// own `dist.tarball`. A git-hosted tarball is not, and trust is gated on
/// the tarball URL rather than the `gitHosted` flag alone.
pub(super) fn is_registry_shaped_resolution(resolution: &LockfileResolution) -> bool {
    match resolution {
        LockfileResolution::Registry(_) => true,
        LockfileResolution::Tarball(tarball) => {
            is_http_tarball_url(&tarball.tarball)
                && !tarball.git_hosted.unwrap_or(false)
                && !is_git_hosted_tarball_url(&tarball.tarball)
        }
        LockfileResolution::Variations(variations) => {
            !variations.variants.is_empty()
                && variations.variants
                    .iter()
                    .all(|variant| is_registry_shaped_resolution(&variant.resolution))
        }
        LockfileResolution::Custom(_) => true,
        LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_) => false,
    }
}

/// Whether a tarball URL uses an HTTP or HTTPS scheme, the only schemes a
/// registry artifact is served over. Case-insensitive to reject a
/// tampered uppercase scheme.
pub(super) fn is_http_tarball_url(url: &str) -> bool {
    url.get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
        || url
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
}

/// Collect every `packages` entry whose resolution violates key/shape
/// invariants (registry-style key backed by non-registry resolution, local
/// `directory` / `file:` tarball under non-`file:` or mismatched `file:` key,
/// or local `git` under non-`git+file:` or mismatched `git+file:` key).
#[must_use]
pub fn collect_resolution_shape_violations(lockfile: &Lockfile) -> Vec<ResolutionPolicyViolation> {
    collect_resolution_shape_violations_with_ctx(lockfile, FileShapeContext::default())
}

pub(super) fn collect_resolution_shape_violations_with_ctx(
    lockfile: &Lockfile,
    shape_ctx: FileShapeContext<'_>,
) -> Vec<ResolutionPolicyViolation> {
    let Some(packages) = lockfile.packages.as_ref() else {
        return Vec::new();
    };
    let mut violations = Vec::new();
    for (key, metadata) in packages {
        let Some(reason) =
            super::candidates::shape_mismatch_reason(key, &metadata.resolution, shape_ctx)
        else {
            continue;
        };
        let version = match key.suffix.registry_qualified() {
            Some((_, version)) => version.to_string(),
            None => key.suffix.version().to_string(),
        };
        violations.push(ResolutionPolicyViolation {
            name: key.name.clone(),
            version,
            resolution: metadata.resolution.clone(),
            code: RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE,
            reason: reason.to_string(),
        });
    }
    violations
}

/// Reject a lockfile whose `packages` keys violate resolution-shape invariants.
pub fn verify_lockfile_resolution_shapes(lockfile: &Lockfile) -> Result<(), VerifyError> {
    verify_lockfile_resolution_shapes_with_ctx(lockfile, FileShapeContext::default())
}

pub(super) fn verify_lockfile_resolution_shapes_with_ctx(
    lockfile: &Lockfile,
    shape_ctx: FileShapeContext<'_>,
) -> Result<(), VerifyError> {
    let violations = collect_resolution_shape_violations_with_ctx(lockfile, shape_ctx);
    if violations.is_empty() {
        return Ok(());
    }
    Err(super::candidates::build_verification_error(violations))
}

/// Whether `resolution` reads from the local filesystem (`directory` or a
/// `file:` tarball, including inside `variations`).
pub(super) fn has_local_filesystem_resolution(resolution: &LockfileResolution) -> bool {
    match resolution {
        LockfileResolution::Directory(_) => true,
        LockfileResolution::Tarball(tarball) => strip_file_url_prefix(&tarball.tarball).is_some(),
        LockfileResolution::Variations(variations) => variations.variants
            .iter()
            .any(|variant| has_local_filesystem_resolution(&variant.resolution)),
        LockfileResolution::Registry(_)
        | LockfileResolution::Custom(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_) => false,
    }
}

/// Whether every selectable variant of `resolution` is a local filesystem
/// resolution (`directory` or `file:` tarball).
pub(super) fn is_purely_local_filesystem_resolution(resolution: &LockfileResolution) -> bool {
    match resolution {
        LockfileResolution::Directory(_) => true,
        LockfileResolution::Tarball(tarball) => strip_file_url_prefix(&tarball.tarball).is_some(),
        LockfileResolution::Variations(variations) => {
            !variations.variants.is_empty()
                && variations.variants
                    .iter()
                    .all(|variant| is_purely_local_filesystem_resolution(&variant.resolution))
        }
        LockfileResolution::Registry(_)
        | LockfileResolution::Custom(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_) => false,
    }
}

pub(super) fn strip_file_url_prefix(url: &str) -> Option<&str> {
    let head = url.get(.."file:".len())?;
    head.eq_ignore_ascii_case("file:")
        .then(|| &url["file:".len()..])
}

/// Add every alias in `aliases` that fails
/// `is_valid_old_npm_package_name` (the `validForOldPackages` rule the
/// dependency-alias check applies) to `invalid`. Only pass maps
/// whose keys become `node_modules/<alias>` directories, not
/// `overrides`, `patched_dependencies`, or peer dependencies.
fn push_invalid_aliases<'alias>(
    aliases: impl Iterator<Item = &'alias PkgName>,
    invalid: &mut BTreeSet<String>,
) {
    for alias in aliases {
        let alias = alias.to_string();
        if !is_valid_old_npm_package_name(&alias) {
            invalid.insert(alias);
        }
    }
}

/// Collect every lockfile-controlled dependency name that isn't a valid
/// npm package name across:
///
/// - every importer's direct-dependency aliases,
/// - every `packages:` entry's package name,
/// - every snapshot's own package name (the `snapshots:` map key), and
/// - every snapshot's `dependencies` / `optionalDependencies` aliases.
fn collect_invalid_dependency_names(lockfile: &Lockfile) -> BTreeSet<String> {
    let mut invalid = BTreeSet::new();
    for importer in lockfile.importers.values() {
        for deps in
            [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
        {
            push_invalid_aliases(
                deps.iter()
                    .flatten()
                    .map(|(alias, _)| alias),
                &mut invalid,
            );
        }
    }
    if let Some(packages) = lockfile.packages.as_ref() {
        push_invalid_aliases(packages.keys().map(|key| &key.name), &mut invalid);
    }
    let Some(snapshots) = lockfile.snapshots.as_ref() else { return invalid };
    for (key, snapshot) in snapshots {
        push_invalid_aliases(std::iter::once(&key.name), &mut invalid);
        for deps in [&snapshot.dependencies, &snapshot.optional_dependencies] {
            push_invalid_aliases(
                deps.iter()
                    .flatten()
                    .map(|(alias, _)| alias),
                &mut invalid,
            );
        }
    }
    invalid
}

/// Reject a lockfile whose dependency names or aliases are not valid npm
/// package names.
///
/// This is an offline, network-free structural check; it does **not**
/// depend on the resolution-policy verifiers, so it must run on every
/// install, **including under `trustLockfile`**, which disables the
/// policy fan-out. A crafted lockfile alias becomes a filesystem path
/// (`node_modules/<alias>`, a virtual-store slot's inner
/// `node_modules/<name>`, a bin, a hoisted link), and pacquet's
/// [`PkgName`] parser does not validate against npm's package-name
/// rules, so an unchecked `..`/`/`-bearing name escapes the intended
/// directory.
///
/// Surfaces [`VerifyError::InvalidDependencyAlias`]
/// (`ERR_PNPM_INVALID_DEPENDENCY_NAME`) listing every offender.
pub fn verify_lockfile_dependency_names(lockfile: &Lockfile) -> Result<(), VerifyError> {
    let invalid = collect_invalid_dependency_names(lockfile);
    if invalid.is_empty() {
        return Ok(());
    }
    let invalid: Vec<String> = invalid.into_iter().collect();
    Err(VerifyError::invalid_dependency_aliases(&invalid))
}

/// Rejects a lockfile when an importer dependency references a snapshot key
/// that does not exist in `snapshots`.
pub fn verify_lockfile_importer_snapshot_links(lockfile: &Lockfile) -> Result<(), VerifyError> {
    lockfile
        .verify_importer_snapshot_links()
        .map_err(|error| VerifyError::missing_dependency(error.0))
}
