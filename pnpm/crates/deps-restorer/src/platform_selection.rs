//! The platforms a `variations` resolution selects an archive for: the
//! host's, the one `supportedArchitectures` names for a runtime, and the one
//! a registry package links
//! ([platform variants](https://github.com/pnpm/rfcs/blob/e920906d80bfb372068eab901fca2bb43fb1b97f/pnpr/text/0000-platform-variants.md#install)).
//! A runtime's archive follows the platform `supportedArchitectures` names
//! first. A registry package's linked variant is the host's, or the one
//! listed platform's when `supportedArchitectures` lists exactly one and
//! omits `current`. The dependency walk, the fetch, the slot hash, and the
//! installability check all read the same choice, so a linked archive is
//! always paired with its own dependencies.

use pnpm_graph_hasher::{host_arch, host_libc, host_platform};
use pnpm_lockfile::{HostFloors, PlatformSelector, VariationsResolution};
use pnpm_package_is_installable::{
    ArchitectureAxes, NamedPlatform, SupportedArchitectures, SupportedPlatform,
};

/// Build the host's [`PlatformSelector`] for runtime-variant
/// matching, from the host's os, cpu, and libc (the latter only on
/// Linux).
///
/// Translating `host_libc()`'s `"unknown"` to `None` lets
/// [`select_platform_variant`](pnpm_lockfile::select_platform_variant)'s asymmetric libc rule apply
/// consistently: `None` and `Some("glibc")` both require the
/// variant to omit `libc`, and `Some("musl")` requires an exact
/// match.
#[must_use]
pub fn host_platform_selector() -> PlatformSelector {
    let libc = match host_libc() {
        "unknown" => None,
        other => Some(other.to_string()),
    };
    PlatformSelector {
        os: host_platform().to_string(),
        cpu: host_arch().to_string(),
        libc,
        floors: None,
    }
}
/// Resolve the runtime archive selector from `supportedArchitectures`.
///
/// Exactly one archive is installed per runtime, so the host's own
/// platform is preferred: an archive built for another platform cannot
/// run here.
///
/// <https://github.com/pnpm/pnpm/issues/13898>
#[must_use]
pub fn runtime_platform_selector(supported: Option<&SupportedArchitectures>) -> PlatformSelector {
    let host = host_platform_selector();
    match supported {
        Some(SupportedArchitectures::Platforms(platforms)) => runtime_platform_of(platforms, host),
        Some(SupportedArchitectures::Axes(axes)) => with_host_floors(
            PlatformSelector {
                os: pick_supported(axes.os.as_deref(), Some(&host.os))
                    .unwrap_or(&host.os)
                    .to_string(),
                cpu: pick_supported(axes.cpu.as_deref(), Some(&host.cpu))
                    .unwrap_or(&host.cpu)
                    .to_string(),
                libc: pick_supported(axes.libc.as_deref(), host.libc.as_deref())
                    .map(str::to_string),
                floors: Some(HostFloors::default()),
            },
            &host,
        ),
        None => host,
    }
}

/// The archive a platform list asks for: this machine's own platform
/// when the list names it, and the first platform it names otherwise.
///
/// A C library pnpm could not read leaves the host without one, so it
/// matches no Linux platform and the first named one is taken. That is
/// the right way to be wrong: which C library this machine has is
/// exactly what could not be determined, and a platform the list names
/// is at least one the workspace prepared for.
pub(crate) fn runtime_platform_of(
    platforms: &[SupportedPlatform],
    host: PlatformSelector,
) -> PlatformSelector {
    let mut first = None;
    for platform in platforms {
        let SupportedPlatform::Named(named) = platform else { return host };
        let selector = PlatformSelector {
            os: named.os().to_string(),
            cpu: named.cpu().to_string(),
            libc: named.libc().map(str::to_string),
            floors: Some(HostFloors::default()),
        };
        if is_host(&selector, &host) {
            return host;
        }
        first.get_or_insert(selector);
    }
    first.unwrap_or(host)
}
/// Whether `selector` names the host's own platform.
fn is_host(selector: &PlatformSelector, host: &PlatformSelector) -> bool {
    selector.os == host.os && selector.cpu == host.cpu && selector.libc == host.libc
}

/// `selector` with the running host's floors, probed on demand, when it
/// names the host's platform: another platform's versions are not known, so
/// its floors all match.
fn with_host_floors(mut selector: PlatformSelector, host: &PlatformSelector) -> PlatformSelector {
    if is_host(&selector, host) {
        selector.floors = host.floors;
    }
    selector
}

pub(crate) fn pick_supported<'a>(
    requested: Option<&'a [String]>,
    host_value: Option<&'a str>,
) -> Option<&'a str> {
    let Some(requested) = requested.filter(|requested| !requested.is_empty()) else {
        return host_value;
    };
    if requested
        .iter()
        .any(|value| value == "current" || Some(value.as_str()) == host_value)
    {
        return host_value;
    }
    requested.first().map(String::as_str)
}

/// The platform each kind of `variations` resolution selects an archive for,
/// derived once from `supportedArchitectures`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantPlatforms {
    /// A runtime's (Node.js, Bun, Deno) archive.
    pub runtime: PlatformSelector,
    /// A registry package's linked variant.
    pub linked: PlatformSelector,
}

impl VariantPlatforms {
    /// The platforms `supported` selects for.
    #[must_use]
    pub fn of(supported: Option<&SupportedArchitectures>) -> Self {
        VariantPlatforms {
            runtime: runtime_platform_selector(supported),
            linked: linked_platform(supported),
        }
    }

    /// The host's platform for both, as without `supportedArchitectures`.
    #[must_use]
    pub fn host() -> Self {
        VariantPlatforms::of(None)
    }

    /// The platform `variations` selects an archive for.
    #[must_use]
    pub fn for_variations(&self, variations: &VariationsResolution) -> &PlatformSelector {
        if variations.is_runtime() { &self.runtime } else { &self.linked }
    }
}

/// The platform a registry package's linked variant fits: the one
/// [`listed_link_platform`] names, and otherwise the host's.
fn linked_platform(supported: Option<&SupportedArchitectures>) -> PlatformSelector {
    match listed_link_platform(supported) {
        Some(listed) => listed,
        None => host_platform_selector(),
    }
}

/// The one platform `supported` names, when it names exactly one and not
/// `current`: a registry package links that platform's variant instead of
/// the host's. A list names one platform by one entry. The `os`, `cpu`, and
/// `libc` form names one when `os` and `cpu` each hold one value and no axis
/// says `current`; an unset `libc` is the host's C library, as everywhere
/// else in that form, and does not keep it from naming one platform.
#[must_use]
pub fn listed_link_platform(
    supported: Option<&SupportedArchitectures>,
) -> Option<PlatformSelector> {
    let supported = supported?;
    let names_a_platform = match supported {
        SupportedArchitectures::Platforms(listed) => {
            matches!(listed.as_slice(), [SupportedPlatform::Named(_)])
        }
        SupportedArchitectures::Axes(axes) => axes_name_a_platform(axes),
    };
    if !names_a_platform {
        return None;
    }
    let host = host_platform_selector();
    let host_libc = host.libc.as_deref().unwrap_or("unknown");
    match supported.platforms(&host.os, &host.cpu, host_libc).as_slice() {
        [named] => Some(named_platform(named, &host)),
        _ => None,
    }
}

/// Whether `axes` set `os` and `cpu` and say `current` on no axis, so they
/// can name a platform other than the host.
fn axes_name_a_platform(axes: &ArchitectureAxes) -> bool {
    let set = |axis: &Option<Vec<String>>| {
        axis.as_ref()
            .is_some_and(|values| !values.is_empty())
    };
    let names_current = [&axes.os, &axes.cpu, &axes.libc]
        .into_iter()
        .flatten()
        .flatten()
        .any(|value| value == "current");
    set(&axes.os) && set(&axes.cpu) && !names_current
}

/// The platforms `supported` names, as [`SupportedArchitectures::platforms`]
/// reads them: `current` and an unset axis are the host's. None when it is
/// not set.
pub(crate) fn listed_platforms(
    supported: Option<&SupportedArchitectures>,
) -> Vec<PlatformSelector> {
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
/// floors and ABI it states known, so an unstated one is not met for
/// linking.
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
    if names_host(&selector, host) && selector.floors == Some(HostFloors::default()) {
        return host.clone();
    }
    selector
}

/// Whether `selector` names the host's platform, glibc spelled or not.
fn names_host(selector: &PlatformSelector, host: &PlatformSelector) -> bool {
    let musl = |libc: Option<&str>| libc == Some("musl");
    selector.os == host.os
        && selector.cpu == host.cpu
        && musl(selector.libc.as_deref()) == musl(host.libc.as_deref())
}

/// What the workspace state records as the linked platform: the `os`,
/// `cpu`, and C library of the platform `supported` links registry
/// packages' variants for.
#[must_use]
pub fn linked_platform_record(supported: Option<&SupportedArchitectures>) -> String {
    let linked = linked_platform(supported);
    let mut record = format!("{}-{}", linked.os, linked.cpu);
    if let Some(libc) = &linked.libc {
        record.push('-');
        record.push_str(libc);
    }
    record
}

/// [`linked_platform_record`] followed by the versions the platform's floors
/// and ABI are matched against: those it states, or the host's, probed. A
/// probe spawns a process, so this is recorded only for a lockfile with a
/// variant that states a floor or an ABI (see [`has_variant_with_floor`]).
#[must_use]
pub fn linked_platform_record_with_floors(supported: Option<&SupportedArchitectures>) -> String {
    let floors = match linked_platform(supported).floors {
        Some(stated) => stated,
        None => pnpm_lockfile::host_floors(),
    };
    format!("{} {floors:?}", linked_platform_record(supported))
}

/// Whether `lockfile` has a registry package published with platform
/// variants, whose linked variant depends on the platform.
#[must_use]
pub fn has_registry_variants(lockfile: &pnpm_lockfile::Lockfile) -> bool {
    lockfile.packages
        .iter()
        .flat_map(|packages| packages.values())
        .any(|metadata| {
            matches!(
                &metadata.resolution,
                pnpm_lockfile::LockfileResolution::Variations(variations)
                    if !variations.is_runtime(),
            )
        })
}

/// Whether a registry package's variant in `lockfile` states a floor or an
/// ABI.
#[must_use]
pub fn has_variant_with_floor(lockfile: &pnpm_lockfile::Lockfile) -> bool {
    lockfile.packages
        .iter()
        .flat_map(|packages| packages.values())
        .filter_map(|metadata| match &metadata.resolution {
            pnpm_lockfile::LockfileResolution::Variations(variations)
                if !variations.is_runtime() =>
            {
                Some(variations)
            }
            _ => None,
        })
        .flat_map(|variations| &variations.variants)
        .flat_map(|variant| &variant.targets)
        .any(|target| {
            target.libc_version.is_some()
                || target.os_version.is_some()
                || target.napi.is_some()
                || target.node_abi.is_some()
        })
}

#[cfg(test)]
mod tests;
