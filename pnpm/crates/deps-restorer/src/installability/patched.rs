use super::platform::{check_installability, manifest_from_metadata};
use pnpm_lockfile::{PackageKey, PackageMetadata, SnapshotEntry};
use pnpm_package_is_installable::{InstallabilityError, InstallabilityOptions};

/// Returns whether the snapshot has an applied patch.
///
/// Only the package's own `(patch_hash=...)` segment counts. A patched peer
/// nested in the peers suffix leaves the package itself unpatched.
pub(crate) fn snapshot_is_patched(
    snapshot_key: &PackageKey,
    snapshot: Option<&SnapshotEntry>,
) -> bool {
    snapshot.is_some_and(|snapshot| snapshot.patched == Some(true))
        || pnpm_deps_path::index_of_dep_path_suffix(&snapshot_key.to_string())
            .patch_hash_index
            .is_some()
}

/// Checks package installability with engine constraints cleared.
pub(super) fn without_published_engines(
    metadata_key: &PackageKey,
    metadata: &PackageMetadata,
    optional: bool,
    base: &InstallabilityOptions<'_>,
) -> Result<Option<InstallabilityError>, Box<InstallabilityError>> {
    let mut manifest = manifest_from_metadata(metadata_key, metadata);
    manifest.engines = None;
    let options = InstallabilityOptions { optional, ..*base };
    check_installability(&metadata_key.to_string(), &manifest, &options)
}
