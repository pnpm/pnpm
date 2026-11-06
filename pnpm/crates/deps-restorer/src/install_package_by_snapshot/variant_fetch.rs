//! The archive a `variations` resolution installs on a host, and how it is
//! fetched.

use super::{InstallPackageBySnapshotError, runtime::render_variant_targets};
use pnpm_config::Config;
use pnpm_lockfile::{
    BinaryResolution, LockfileResolution, PackageKey, PlatformAssetResolution, PlatformSelector,
};

/// How the archive a `variations` resolution selects is fetched.
pub(super) enum VariantFetch<'a> {
    /// A runtime archive (Node.js, Bun, Deno).
    Binary(&'a BinaryResolution),
    /// A registry package's archive: a platform variant, or the variant that
    /// targets `any`.
    Tarball { resolution: LockfileResolution, source: ArchiveSource },
}

/// Which archive of a registry package version is fetched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ArchiveSource {
    /// A platform variant, verified against its tar's digest and served
    /// without redirects, like a revision.
    Variant,
    /// The version's archive that every host can install: the variant that
    /// targets `any`, or the one archive of a version without variants.
    Any,
}

/// The archive `variations` installs on `platform`: the variant that fits
/// it best, or the variant that targets `any`.
///
/// Runtimes and registry packages share this selection. The selected
/// resolution's type decides how it is fetched: a `binary` archive goes
/// through the runtime extractor, and a registry or tarball resolution
/// through the package tarball fetcher, a variant's integrity-only entry
/// at its registry route rebuilt from the package key and its `tag`.
pub(super) fn variant_for_host<'a>(
    variations: &'a pnpm_lockfile::VariationsResolution,
    package_key: &PackageKey,
    platform: &PlatformSelector,
    config: &Config,
) -> Result<(&'a PlatformAssetResolution, VariantFetch<'a>), InstallPackageBySnapshotError> {
    let selected_target = || {
        format!("os = `{}`, cpu = `{}`, libc = `{:?}`", platform.os, platform.cpu, platform.libc)
    };
    let selected = variations
        .select(platform)
        .map_err(|_| InstallPackageBySnapshotError::AmbiguousPlatformVariant {
            package_key: package_key.to_string(),
            selected_target: selected_target(),
        })?;
    let Some(selected) = selected else {
        return Err(InstallPackageBySnapshotError::NoMatchingPlatformVariant {
            package_key: package_key.to_string(),
            selected_target: selected_target(),
            available_targets: render_variant_targets(&variations.variants),
        });
    };
    Ok((selected, archive_fetch(selected, variations, package_key, config)?))
}

/// How `selected`, one archive of `variations`, is fetched.
pub(super) fn archive_fetch<'a>(
    selected: &'a PlatformAssetResolution,
    variations: &pnpm_lockfile::VariationsResolution,
    package_key: &PackageKey,
    config: &Config,
) -> Result<VariantFetch<'a>, InstallPackageBySnapshotError> {
    let source = if selected.is_any() { ArchiveSource::Any } else { ArchiveSource::Variant };
    let resolution = match &selected.resolution {
        LockfileResolution::Binary(binary) => return Ok(VariantFetch::Binary(binary)),
        LockfileResolution::Registry(registry) if !selected.is_any() => {
            variant_registry_tarball(selected, registry, variations.revision, package_key, config)?
        }
        resolution @ (LockfileResolution::Registry(_) | LockfileResolution::Tarball(_)) => {
            resolution.clone()
        }
        other => {
            return Err(InstallPackageBySnapshotError::UnsupportedVariantResolution {
                package_key: package_key.to_string(),
                inner_kind: resolution_kind(other),
            });
        }
    };
    Ok(VariantFetch::Tarball { resolution, source })
}

/// A registry package variant recorded integrity-only, at the registry
/// route `<registry>/<name>@<version>[+r<revision>]/variants/<tag>`, its tag
/// derived from its targets.
fn variant_registry_tarball(
    variant: &PlatformAssetResolution,
    registry: &pnpm_lockfile::RegistryResolution,
    revision: Option<u64>,
    package_key: &PackageKey,
    config: &Config,
) -> Result<LockfileResolution, InstallPackageBySnapshotError> {
    let invalid_tag = || {
        super::tarball_resolution::invalid_tarball_revision(
            package_key,
            "has a registry variant without a valid tag",
        )
    };
    let tag = variant.variant_tag().ok_or_else(invalid_tag)?;
    let (registry_url, version) =
        super::tarball_resolution::registry_and_version(package_key, config)?;
    let tarball = pnpm_lockfile::variant_registry_tarball_url(
        &package_key.name.to_string(),
        &version,
        &tag,
        revision,
        &registry_url,
    )
    .ok_or_else(invalid_tag)?;
    Ok(LockfileResolution::Tarball(pnpm_lockfile::TarballResolution {
        tarball,
        integrity: Some(registry.integrity.clone()),
        revision: None,
        git_hosted: None,
        path: None,
    }))
}

fn resolution_kind(resolution: &LockfileResolution) -> &'static str {
    match resolution {
        LockfileResolution::Tarball(_) => "tarball",
        LockfileResolution::Registry(_) => "registry",
        LockfileResolution::Directory(_) => "directory",
        LockfileResolution::Git(_) => "git",
        LockfileResolution::Variations(_) => "variations",
        LockfileResolution::Custom(_) => "custom",
        LockfileResolution::Binary(_) => "binary",
    }
}
