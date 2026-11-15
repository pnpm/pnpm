use std::{collections::BTreeSet, sync::Arc};

use pnpm_lockfile::{Lockfile, LockfileResolution, PkgName, is_git_hosted_tarball_url};
use pnpm_package_name::is_valid_old_npm_package_name;
use pnpm_resolving_resolver_base::{
    ResolutionVerification, ResolutionVerifier, VerifyCtx, VerifyFuture,
};

use super::VerifyLockfileResolutionsOptions;
use crate::errors::VerifyError;

pub const RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE: &str = "RESOLUTION_SHAPE_MISMATCH";

pub(super) fn verify_offline_structural_checks(lockfile: &Lockfile) -> Result<(), VerifyError> {
    verify_lockfile_dependency_names(lockfile)?;
    verify_lockfile_importer_snapshot_links(lockfile)?;
    Ok(())
}

pub(super) fn verify_structure_before_install(
    lockfile: &Lockfile,
    opts: &VerifyLockfileResolutionsOptions<'_>,
) -> Result<(), VerifyError> {
    if opts.repairs_lockfile {
        return verify_lockfile_dependency_names(lockfile);
    }
    verify_offline_structural_checks(lockfile)
}

/// Cache-key participant for an always-on offline structural check
/// (resolution-shape, dependency-alias): a record written before the
/// check's rule existed lacks its `flag`, so `can_trust_past_check`
/// rejects it and forces a re-verification. `verify` is never invoked —
/// the identity is appended only to the verifier lists handed to the
/// cache lookup and recorder.
struct OfflineCheckCacheIdentity {
    policy: serde_json::Map<String, serde_json::Value>,
    flag: &'static str,
}

fn resolution_shape_cache_identity() -> Arc<dyn ResolutionVerifier> {
    let mut policy = serde_json::Map::new();
    policy.insert("resolutionShapeCheck".to_string(), serde_json::Value::Bool(true));
    Arc::new(OfflineCheckCacheIdentity { policy, flag: "resolutionShapeCheck" })
}

fn dependency_alias_cache_identity() -> Arc<dyn ResolutionVerifier> {
    let mut policy = serde_json::Map::new();
    policy.insert("dependencyAliasCheck".to_string(), serde_json::Value::Bool(true));
    Arc::new(OfflineCheckCacheIdentity { policy, flag: "dependencyAliasCheck" })
}

fn variation_resolution_cache_identity() -> Arc<dyn ResolutionVerifier> {
    let mut policy = serde_json::Map::new();
    policy.insert("variationResolutionCheck".to_string(), serde_json::Value::Bool(true));
    Arc::new(OfflineCheckCacheIdentity { policy, flag: "variationResolutionCheck" })
}

/// Every verifier list that flows into the verification cache must
/// carry the always-on offline structural checks' identities, so a
/// record written before one of those rules existed cannot
/// stat-fast-path around it — its missing flag fails
/// `can_trust_past_check`, forcing a re-verification that runs the new
/// check. Used by the gate itself and by
/// [`crate::record_lockfile_verified()`], whose freshly-resolved
/// lockfile satisfies these invariants by construction (the resolver
/// validates aliases at manifest-read time and derives every resolution
/// key from the resolution it just produced).
pub(crate) fn with_offline_check_cache_identities(
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> Vec<Arc<dyn ResolutionVerifier>> {
    verifiers
        .iter()
        .cloned()
        .chain([
            resolution_shape_cache_identity(),
            dependency_alias_cache_identity(),
            variation_resolution_cache_identity(),
        ])
        .collect()
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
        cached.get(self.flag) == Some(&serde_json::Value::Bool(true))
    }
}

/// Whether a resolution is registry-shaped. A plain tarball
/// resolution is registry-shaped because the npm verifier unconditionally
/// binds explicit tarball URLs of semver-keyed entries to the registry's
/// own `dist.tarball`. A git-hosted tarball is not — and trust is gated on
/// the tarball URL rather than the `gitHosted` flag alone.
pub(super) fn is_registry_shaped_resolution(resolution: &LockfileResolution) -> bool {
    match resolution {
        LockfileResolution::Registry(_) => true,
        LockfileResolution::Tarball(tarball) => {
            // The tarball URL must be an HTTP or HTTPS registry artifact and not
            // git-hosted. The npm verifier's tarball-URL binding skips
            // schemes other than HTTP and HTTPS (file:, etc.), so a `file:` tarball under a
            // name@semver key would otherwise be trusted with no safety net.
            is_http_tarball_url(&tarball.tarball)
                && tarball.git_hosted != Some(true)
                && !is_git_hosted_tarball_url(&tarball.tarball)
        }
        LockfileResolution::Variations(variations) => {
            !variations.variants.is_empty()
                && variations.resolutions().all(is_registry_shaped_resolution)
        }
        // Custom resolver protocols (`type: "custom:*"`) can only be materialized
        // by a project-configured custom fetcher, which owns its own trust
        // decision (allowBuilds included).
        LockfileResolution::Custom(_) => true,
        LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_) => false,
    }
}

/// Whether a tarball URL uses an HTTP or HTTPS scheme — the only schemes a
/// registry artifact is served over. Case-insensitive to reject a
/// tampered uppercase scheme.
fn is_http_tarball_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

/// Add every alias in `aliases` that fails
/// `is_valid_old_npm_package_name` (the `validForOldPackages` rule the
/// dependency-alias check applies) to `invalid`. Only pass maps
/// whose keys become `node_modules/<alias>` directories — not
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
/// npm package name. Three sources, each of which becomes a
/// `node_modules/<name>` path component at install time:
///
/// - every importer's direct-dependency aliases,
/// - every snapshot's own package name (the `snapshots:` map key), and
/// - every snapshot's `dependencies` / `optionalDependencies` aliases.
///
/// A name carrying a `..` traversal, a `/`, or a reserved value such as
/// `node_modules` / `.bin` / `.pnpm` could make an install write outside
/// the intended directory or overwrite pnpm-owned layout.
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
/// This is an offline, network-free structural check — it does **not**
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
        .map_err(|error| VerifyError::missing_dependency(&error.0))
}
