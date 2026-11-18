//! Fetching into the store, without linking, the archives that
//! `supportedArchitectures` keeps for its platforms beside the ones the
//! install links, and the packages those archives depend on
//! ([platform variants](https://github.com/pnpm/rfcs/blob/e920906d80bfb372068eab901fca2bb43fb1b97f/pnpr/text/0000-platform-variants.md#install)).
//! [`crate::platform_archives`] decides which archives they are. An archive
//! the store holds already is read from the store index, not downloaded.

use super::{
    super::{
        InstallPackageBySnapshot, InstallPackageBySnapshotError, host_platform_selector,
        variant_fetch::{VariantFetch, archive_fetch},
    },
    TarballFetch,
};
use pnpm_lockfile::{HostFloors, PlatformAssetResolution, PlatformSelector, VariationsResolution};
use pnpm_package_is_installable::{NamedPlatform, SupportedArchitectures};

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

/// The platforms `supported` names, as [`SupportedArchitectures::platforms`]
/// reads them: `current` and an unset axis are the host's. None when it is
/// not set, so only the linked archive is fetched.
pub(super) fn fetch_platforms(supported: Option<&SupportedArchitectures>) -> Vec<PlatformSelector> {
    let Some(supported) = supported else {
        return Vec::new();
    };
    let host = host_platform_selector();
    let host_libc = host.libc.as_deref().unwrap_or("unknown");
    supported
        .platforms(&host.os, &host.cpu, host_libc)
        .iter()
        .map(|named| named_platform(named, &host))
        .collect()
}

/// `named` as a selector: the host's, whose floors are probed, when it names
/// the host's platform and states no floor, and otherwise with only the
/// floors and ABI it states known.
fn named_platform(named: &NamedPlatform, host: &PlatformSelector) -> PlatformSelector {
    let stated = &named.platform;
    let selector = PlatformSelector {
        os: named.os().to_string(),
        cpu: named.cpu().to_string(),
        libc: named.libc().map(str::to_string),
        floors: Some(HostFloors {
            libc_version: stated.libc_version,
            os_version: stated.os_version,
            napi: stated.napi,
            node_abi: stated.node_abi,
        }),
    };
    if is_host(&selector, host) && selector.floors == Some(HostFloors::default()) {
        return host.clone();
    }
    selector
}

/// Whether `selector` names the host's platform, glibc spelled or not.
fn is_host(selector: &PlatformSelector, host: &PlatformSelector) -> bool {
    let musl = |libc: Option<&str>| libc == Some("musl");
    selector.os == host.os
        && selector.cpu == host.cpu
        && musl(selector.libc.as_deref()) == musl(host.libc.as_deref())
}

#[cfg(test)]
mod tests;
