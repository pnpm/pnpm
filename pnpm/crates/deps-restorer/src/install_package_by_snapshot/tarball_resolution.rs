use super::InstallPackageBySnapshotError;
use pipe_trait::Pipe;
use pnpm_config::Config;
use pnpm_fs::lexical_normalize;
use pnpm_lockfile::{
    LockfileResolution, PackageKey, TarballUrlOptions, integrity_addressed_registry_tarball_url,
    is_git_hosted_tarball_url, is_integrity_addressed_registry_tarball_url, npm_tarball_url,
    registry_server_type,
};
use pnpm_resolving_npm_resolver::pick_registry_for_package;
use std::{borrow::Cow, collections::HashMap, path::Path};

pub(crate) fn local_file_tarball_install_url<'a>(
    tarball_url: Cow<'a, str>,
    workspace_root: &Path,
) -> Cow<'a, str> {
    let Some(path) = tarball_url.strip_prefix("file:") else {
        return tarball_url;
    };
    if path.starts_with("//") || Path::new(path).is_absolute() {
        return tarball_url;
    }
    Cow::Owned(format!("file:{}", lexical_normalize(&workspace_root.join(path)).display()))
}
/// Resolve the tarball URL + integrity for tarball- and registry-shaped
/// resolutions. Factored out so the per-resolution-type dispatch in
/// [`InstallPackageBySnapshot::run`](crate::InstallPackageBySnapshot::run) reads top-down: each variant builds
/// its own `cas_paths`. Public because the pnpr server derives the same
/// URLs when it announces a verified frozen lockfile's tarballs to the
/// client — both sides must derive byte-identical URLs so the client's
/// prefetch mem-cache keys line up.
///
/// The integrity is `None` only for the shapes
/// [`unverified_fetch_is_allowed`] exempts — every other resolution whose
/// recorded integrity pins nothing (absent, or the empty SRI string an
/// edited lockfile can carry) is refused here rather than fetched
/// unchecked. See
/// [`pnpm_tarball::IngestTarballToStore::package_integrity`] for
/// what an unverified fetch does.
///
/// # Panics
///
/// On directory / git / binary / variations resolutions — callers gate
/// on the tarball/registry shapes first.
pub fn tarball_url_and_integrity<'a>(
    resolution: &'a LockfileResolution,
    package_key: &PackageKey,
    config: &'a Config,
) -> Result<(Cow<'a, str>, Option<&'a ssri::Integrity>), InstallPackageBySnapshotError> {
    match resolution {
        LockfileResolution::Tarball(tarball_resolution) => {
            tarball_resolution_url(resolution, tarball_resolution, package_key, config)
        }
        LockfileResolution::Registry(registry_resolution) => {
            registry_resolution_url(resolution, registry_resolution, package_key, config)
        }
        // Caller (`run`) only invokes this helper for the tarball /
        // registry arms; git, directory, binary, variations, and
        // custom resolutions never reach here.
        LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_)
        | LockfileResolution::Variations(_)
        | LockfileResolution::Custom(_) => {
            unreachable!("tarball_url_and_integrity called with non-tarball resolution");
        }
    }
}
pub(super) fn tarball_resolution_url<'a>(
    resolution: &'a LockfileResolution,
    tarball_resolution: &'a pnpm_lockfile::TarballResolution,
    package_key: &PackageKey,
    config: &Config,
) -> Result<(Cow<'a, str>, Option<&'a ssri::Integrity>), InstallPackageBySnapshotError> {
    let tarball_url = tarball_resolution.tarball.as_str();
    let integrity = resolution.checkable_integrity();
    if tarball_resolution.revision.is_some() {
        check_tarball_revision(tarball_url, integrity, tarball_resolution, package_key, config)?;
    }
    if integrity.is_none() && !unverified_fetch_is_allowed(tarball_url) {
        return Err(InstallPackageBySnapshotError::MissingTarballIntegrity {
            package_key: package_key.to_string(),
        });
    }
    Ok((tarball_url.pipe(Cow::Borrowed), integrity))
}
/// A `revision`-carrying tarball resolution addresses a registry
/// tarball by its integrity, so the recorded URL must be exactly the
/// one that integrity derives against the package's registry.
pub(super) fn check_tarball_revision(
    tarball_url: &str,
    integrity: Option<&ssri::Integrity>,
    tarball_resolution: &pnpm_lockfile::TarballResolution,
    package_key: &PackageKey,
    config: &Config,
) -> Result<(), InstallPackageBySnapshotError> {
    if tarball_url.starts_with("file:") || tarball_resolution.is_git_hosted() {
        return Err(invalid_tarball_revision(package_key, "does not identify a registry tarball"));
    }
    let Some(integrity) = integrity else {
        return Err(invalid_tarball_revision(package_key, "has invalid or missing integrity"));
    };
    let (registry, _) = registry_and_version(package_key, config)?;
    if !is_integrity_addressed_registry_tarball_url(tarball_url, integrity, &registry) {
        return Err(invalid_tarball_revision(package_key, "has a mismatched tarball URL"));
    }
    Ok(())
}
pub(super) fn registry_resolution_url<'a>(
    resolution: &'a LockfileResolution,
    registry_resolution: &pnpm_lockfile::RegistryResolution,
    package_key: &PackageKey,
    config: &Config,
) -> Result<(Cow<'a, str>, Option<&'a ssri::Integrity>), InstallPackageBySnapshotError> {
    let Some(integrity) = resolution.checkable_integrity() else {
        if registry_resolution.revision.is_some() {
            return Err(invalid_tarball_revision(package_key, "has invalid or missing integrity"));
        }
        return Err(InstallPackageBySnapshotError::MissingTarballIntegrity {
            package_key: package_key.to_string(),
        });
    };
    let (registry, version) = registry_and_version(package_key, config)?;
    let tarball_url = match registry_resolution.revision {
        Some(_) => {
            integrity_addressed_registry_tarball_url(integrity, &registry).ok_or_else(|| {
                invalid_tarball_revision(package_key, "has invalid or missing integrity")
            })?
        }
        None => npm_tarball_url(
            &package_key.name.to_string(),
            &version,
            TarballUrlOptions {
                registry: &registry,
                server_type: registry_server_type(&config.registry_options_by_url, &registry),
            },
        ),
    };
    Ok((Cow::Owned(tarball_url), Some(integrity)))
}
pub(super) fn registry_and_version(
    package_key: &PackageKey,
    config: &Config,
) -> Result<(String, String), InstallPackageBySnapshotError> {
    if let Some((registry_name, version)) = package_key.suffix.registry_qualified() {
        let builtin = pnpm_resolving_npm_resolver::BUILTIN_REGISTRIES_BY_PREFIX
            .iter()
            .find(|(name, _)| *name == registry_name)
            .map(|(_, url)| (*url).to_string());
        let registry =
            config.registries_by_prefix.get(registry_name).cloned().or(builtin).ok_or_else(
                || InstallPackageBySnapshotError::MissingNamedRegistry {
                    package_key: package_key.to_string(),
                    registry_name: registry_name.to_string(),
                },
            )?;
        return Ok((registry, version.to_string()));
    }
    let name = package_key.name.to_string();
    let registries: HashMap<String, String> = config.resolved_registries().into_iter().collect();
    Ok((
        pick_registry_for_package(&registries, &name, None),
        package_key.suffix.version().to_string(),
    ))
}
pub(super) fn invalid_tarball_revision(
    package_key: &PackageKey,
    reason: &'static str,
) -> InstallPackageBySnapshotError {
    InstallPackageBySnapshotError::InvalidTarballRevision {
        package_key: package_key.to_string(),
        reason,
    }
}
/// Whether a tarball resolution that records no `integrity` may still
/// be fetched.
///
/// pnpm exempts the two shapes it never recorded a hash for, keyed off
/// the URL the way `classifyResolution` does — a lockfile's own
/// `gitHosted` marker is only a hint:
///
/// - a git-host archive URL, which pins a full commit SHA (older pnpm
///   versions wrote these without an `integrity`), and
/// - a `file:` tarball, which is local to the project.
///
/// Every other remote tarball must carry one, so bytes fetched over
/// the network for a package the lockfile claims to pin stay
/// verifiable.
#[must_use]
pub fn unverified_fetch_is_allowed(tarball_url: &str) -> bool {
    tarball_url.starts_with("file:") || is_git_hosted_tarball_url(tarball_url)
}
