//! The archives of a registry package version that `supportedArchitectures`
//! keeps in the store beside the one the install links
//! ([platform variants](https://github.com/pnpm/rfcs/blob/e920906d80bfb372068eab901fca2bb43fb1b97f/pnpr/text/0000-platform-variants.md#install)):
//! every variant that matches one of its platforms, and the archive that
//! targets `any` for a platform no variant can be linked on. They are
//! fetched for `pnpm fetch`, Docker images, and bundles built for another
//! machine, and are not linked.

use super::{
    super::{
        InstallPackageBySnapshot, InstallPackageBySnapshotError, host_platform_selector,
        variant_fetch::{VariantFetch, archive_fetch},
    },
    SnapshotFetch, TarballFetch,
};
use pnpm_lockfile::{HostFloors, PlatformAssetResolution, PlatformSelector, VariationsResolution};
use pnpm_package_is_installable::{NamedPlatform, SupportedArchitectures};

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
