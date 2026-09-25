use super::platform::{check_installability, manifest_from_metadata};
use pnpm_lockfile::{PackageKey, PackageMetadata};
use pnpm_package_is_installable::{InstallabilityError, InstallabilityOptions};

/// Published `engines` are omitted. The build phase checks the patched manifest.
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
