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
use crate::StoredArchive;
use pnpm_lockfile::{LockfileResolution, PackageKey, PackageMetadata};
use pnpm_reporter::Reporter;
use std::collections::HashMap;

impl InstallPackageBySnapshot<'_> {
    /// Fetch each of `archives` into the store, the client's per-host
    /// limit bounding how many download at once.
    pub async fn store_archives<Reporter: self::Reporter>(
        &self,
        archives: &[(PackageKey, StoredArchive)],
        packages: &HashMap<PackageKey, PackageMetadata>,
    ) -> Result<(), InstallPackageBySnapshotError> {
        let allow_build = self.allow_build();
        let mut downloads = Vec::new();
        for (key, archive) in archives {
            let Some(metadata) = packages.get(&key.without_peer()) else { continue };
            if let Some((resolution, source)) = stored_fetch(key, metadata, *archive, self)? {
                downloads.push((key, metadata, resolution, source));
            }
        }
        let allow_build = &allow_build;
        futures_util::future::try_join_all(
            downloads
                .into_iter()
                .map(|(key, metadata, resolution, source)| async move {
                    let package_id = key.pkg_id();
                    let download = self.ingest(metadata, &package_id);
                    self.tarball_cas_paths::<Reporter>(TarballFetch {
                        download: &download,
                        resolution: &resolution,
                        package_key: key,
                        package_id: &package_id,
                        allow_build,
                        source,
                    })
                    .await
                }),
        )
        .await
        .map(drop)
    }
}

/// The resolution and source `archive` of `metadata` downloads from, `None`
/// for one the store does not keep this way, such as a runtime's binary.
fn stored_fetch(
    key: &PackageKey,
    metadata: &PackageMetadata,
    archive: StoredArchive,
    installer: &InstallPackageBySnapshot<'_>,
) -> Result<Option<(LockfileResolution, ArchiveSource)>, InstallPackageBySnapshotError> {
    match (archive, &metadata.resolution) {
        (StoredArchive::Variant(position), LockfileResolution::Variations(variations)) => {
            let Some(variant) = variations.variants.get(position) else { return Ok(None) };
            Ok(match archive_fetch(variant, variations, key, installer.ctx.config)? {
                VariantFetch::Tarball { resolution, source } => Some((resolution, source)),
                VariantFetch::Binary(_) => None,
            })
        }
        (
            StoredArchive::Own,
            resolution @ (LockfileResolution::Registry(_) | LockfileResolution::Tarball(_)),
        ) => Ok(Some((resolution.clone(), ArchiveSource::Any))),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests;
