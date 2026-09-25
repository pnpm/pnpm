use super::platform::{check_installability, manifest_from_metadata};
use pnpm_lockfile::{PackageKey, PackageMetadata, SnapshotEntry};
use pnpm_package_is_installable::{InstallabilityError, InstallabilityOptions};

/// Returns whether the snapshot has an applied patch.
pub(super) fn snapshot_is_patched(snapshot_key: &PackageKey, snapshot: &SnapshotEntry) -> bool {
    snapshot.patched == Some(true) || snapshot_key.to_string().contains("(patch_hash=")
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
