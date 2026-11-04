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
use pnpm_lockfile::{
    Floor, HostFloors, PlatformAssetResolution, PlatformSelector, VariationsResolution,
};
use pnpm_package_is_installable::{
    ArchitectureAxes, NamedPlatform, SupportedArchitectures, SupportedPlatform,
};

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

/// The platforms `supported` names: each platform of its list, `current`
/// as the host, or every combination of its `os`, `cpu`, and `libc`. None
/// when it is not set, so only the linked archive is fetched.
pub(super) fn fetch_platforms(supported: Option<&SupportedArchitectures>) -> Vec<PlatformSelector> {
    let host = host_platform_selector();
    let mut platforms = match supported {
        None => return Vec::new(),
        Some(SupportedArchitectures::Platforms(listed)) => listed
            .iter()
            .map(|platform| match platform {
                SupportedPlatform::Current => host.clone(),
                SupportedPlatform::Named(named) => named_platform(named, &host),
            })
            .collect(),
        Some(SupportedArchitectures::Axes(axes)) => axes_platforms(axes, &host),
    };
    platforms.dedup();
    platforms
}

/// `named` as a selector: the host's, whose floors are probed, when it names
/// the host's platform, and otherwise with only the floor it states known.
fn named_platform(named: &NamedPlatform, host: &PlatformSelector) -> PlatformSelector {
    let selector = PlatformSelector {
        os: named.os.name().to_string(),
        cpu: named.architecture.cpu().to_string(),
        libc: named.libc
            .as_ref()
            .map(|libc| libc.name().to_string()),
        floors: Some(HostFloors {
            libc_version: named.libc
                .as_ref()
                .and_then(|libc| libc.baseline.as_deref())
                .and_then(glibc_floor),
            ..HostFloors::default()
        }),
    };
    if is_host(&selector, host) && selector.floors == Some(HostFloors::default()) {
        return host.clone();
    }
    selector
}

/// The glibc floor of a `manylinux_<major>_<minor>` baseline. A
/// `musllinux` baseline states none: no variant has a musl floor.
fn glibc_floor(baseline: &str) -> Option<Floor> {
    let (major, minor) = baseline.strip_prefix("manylinux_")?.split_once('_')?;
    Some(Floor { major: major.parse().ok()?, minor: minor.parse().ok()? })
}

/// Every combination of `axes`, an axis left unset or a `current` value
/// standing for the host's.
fn axes_platforms(axes: &ArchitectureAxes, host: &PlatformSelector) -> Vec<PlatformSelector> {
    let oses = axis_values(axes.os.as_ref(), Some(&host.os));
    let cpus = axis_values(axes.cpu.as_ref(), Some(&host.cpu));
    let libcs = axis_values(axes.libc.as_ref(), host.libc.as_deref());
    let mut platforms = Vec::new();
    for (os, cpu) in oses
        .iter()
        .flatten()
        .flat_map(|os| {
            cpus.iter()
                .flatten()
                .map(move |cpu| (os, cpu))
        })
    {
        platforms.extend(
            libcs
                .iter()
                .map(|libc| axes_platform((os, cpu), libc.as_deref(), host)),
        );
    }
    platforms
}

/// The values of one axis: the host's when it is unset, and each value
/// with `current` read as the host's.
fn axis_values(axis: Option<&Vec<String>>, host_value: Option<&str>) -> Vec<Option<String>> {
    let Some(values) = axis.filter(|values| !values.is_empty()) else {
        return vec![host_value.map(str::to_string)];
    };
    values
        .iter()
        .map(|value| if value == "current" { host_value } else { Some(value.as_str()) })
        .map(|value| value.map(str::to_string))
        .collect()
}

/// One combination of the axes, a C library only on Linux.
fn axes_platform(
    (os, cpu): (&str, &str),
    libc: Option<&str>,
    host: &PlatformSelector,
) -> PlatformSelector {
    let selector = PlatformSelector {
        os: os.to_string(),
        cpu: cpu.to_string(),
        libc: libc
            .filter(|_| os == "linux")
            .map(str::to_string),
        floors: Some(HostFloors::default()),
    };
    if is_host(&selector, host) { host.clone() } else { selector }
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
