//! Fetching into the store, without linking, the archives that
//! `supportedArchitectures` keeps for its platforms beside the ones the
//! install links, and the packages those archives depend on
//! ([platform variants](https://github.com/pnpm/rfcs/blob/e920906d80bfb372068eab901fca2bb43fb1b97f/pnpr/text/0000-platform-variants.md#install)).
//! [`crate::platform_archives`] decides which archives they are. An archive
//! the store holds already is read from the store index, not downloaded.

use super::{
    super::{
        InstallPackageBySnapshot, InstallPackageBySnapshotError,
        variant_fetch::{ArchiveSource, VariantFetch, archive_fetch},
    },
    TarballFetch,
};
use crate::{KeptArchive, StoredArchive};
use pnpm_lockfile::{LockfileResolution, PackageMetadata};
use pnpm_reporter::Reporter;

impl InstallPackageBySnapshot<'_> {
    /// Fetch the archive `kept` of `metadata` into the store, without
    /// preparing or linking it. An archive the store does not keep this way,
    /// such as a runtime's binary, is left out.
    pub async fn store_archive<Reporter: self::Reporter>(
        &self,
        kept: &KeptArchive,
        metadata: &PackageMetadata,
    ) -> Result<(), InstallPackageBySnapshotError> {
        let Some((resolution, source)) = stored_fetch(kept, metadata, self)? else {
            return Ok(());
        };
        let package_id = kept.key.pkg_id();
        let download = self.ingest(metadata, &package_id);
        let allow_build = self.allow_build();
        self.tarball_cas_paths::<Reporter>(TarballFetch {
            download: &download,
            resolution: &resolution,
            package_key: &kept.key,
            package_id: &package_id,
            allow_build: &allow_build,
            source,
        })
        .await
        .map(drop)
    }
}

/// The resolution and source the archive `kept` of `metadata` downloads
/// from, `None` for one the store does not keep this way, such as a
/// runtime's binary.
fn stored_fetch(
    kept: &KeptArchive,
    metadata: &PackageMetadata,
    installer: &InstallPackageBySnapshot<'_>,
) -> Result<Option<(LockfileResolution, ArchiveSource)>, InstallPackageBySnapshotError> {
    match (kept.archive, &metadata.resolution) {
        (StoredArchive::Variant(position), LockfileResolution::Variations(variations)) => {
            let Some(variant) = variations.variants.get(position) else { return Ok(None) };
            Ok(match archive_fetch(variant, variations, &kept.key, installer.ctx.config)? {
                VariantFetch::Tarball { resolution, source } => Some((resolution, source)),
                VariantFetch::Binary(_) => None,
            })
        }
        (StoredArchive::Own, resolution @ LockfileResolution::Registry(_)) => {
            Ok(Some((resolution.clone(), ArchiveSource::Any)))
        }
        (StoredArchive::Own, resolution @ LockfileResolution::Tarball(tarball))
            if !tarball.is_git_hosted() =>
        {
            Ok(Some((resolution.clone(), ArchiveSource::Any)))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests;
