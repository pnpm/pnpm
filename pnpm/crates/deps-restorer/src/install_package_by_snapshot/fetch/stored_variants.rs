//! The archives of a registry package version that `supportedArchitectures`
//! keeps in the store beside the one the install links
//! ([platform variants](https://github.com/pnpm/rfcs/blob/e920906d80bfb372068eab901fca2bb43fb1b97f/pnpr/text/0000-platform-variants.md#install)):
//! every variant that matches one of its platforms, and the archive that
//! targets `any` for a platform no variant can be linked on. They are
//! fetched for `pnpm fetch`, Docker images, and bundles built for another
//! machine, and are not linked.

use super::{
    super::{
        InstallPackageBySnapshot, InstallPackageBySnapshotError,
        variant_fetch::{VariantFetch, archive_fetch},
    },
    SnapshotFetch, TarballFetch,
};
use crate::platform_selection::listed_platforms as fetch_platforms;
use pnpm_lockfile::{PlatformAssetResolution, PlatformSelector, VariationsResolution};

use pnpm_reporter::Reporter;

impl InstallPackageBySnapshot<'_> {
    /// Fetch into the store every archive of `variations` that
    /// `supportedArchitectures` keeps besides `linked`, without linking it.
    pub(super) async fn fetch_stored_variants<Reporter: self::Reporter>(
        &self,
        fetch: &SnapshotFetch<'_>,
        variations: &VariationsResolution,
        linked: &PlatformAssetResolution,
        allow_build: &(impl Fn(&str) -> Option<bool> + Send + Sync),
    ) -> Result<(), InstallPackageBySnapshotError> {
        let supported = self.ctx.config.supported_architectures.as_ref();
        let mut downloads = Vec::new();
        for archive in stored_archives(variations, &fetch_platforms(supported), linked) {
            if let VariantFetch::Tarball { resolution, source } =
                archive_fetch(archive, variations, fetch.package_key, self.ctx.config)?
            {
                downloads.push((resolution, source));
            }
        }
        futures_util::future::try_join_all(
            downloads
                .into_iter()
                .map(|(resolution, source)| async move {
                    self.tarball_cas_paths::<Reporter>(TarballFetch {
                        download: fetch.download,
                        resolution: &resolution,
                        package_key: fetch.package_key,
                        package_id: fetch.package_id,
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

/// The archives of a registry package's `variations` that `platforms` keep
/// in the store, except `linked`, each once.
pub(super) fn stored_archives<'a>(
    variations: &'a VariationsResolution,
    platforms: &[PlatformSelector],
    linked: &PlatformAssetResolution,
) -> Vec<&'a PlatformAssetResolution> {
    if variations.is_runtime() {
        return Vec::new();
    }
    let mut stored: Vec<&PlatformAssetResolution> = Vec::new();
    for archive in platforms
        .iter()
        .flat_map(|platform| variations.fetched_for(platform))
    {
        let known =
            std::ptr::eq(archive, linked) || stored.iter().any(|kept| std::ptr::eq(*kept, archive));
        if !known {
            stored.push(archive);
        }
    }
    stored
}

#[cfg(test)]
mod tests;
