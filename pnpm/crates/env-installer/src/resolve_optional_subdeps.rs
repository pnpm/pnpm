//! Resolve one level of a config dependency's `optionalDependencies`
//! into the env lockfile.
//!
//! Only exact versions are accepted: a range or tag would let the
//! resolved version drift between machines even with a stable parent
//! integrity, breaking the lockfile's reproducibility promise.

use crate::{
    ConfigDepError, manifest_lockfile::package_metadata, options::ConfigDepsInstallOptions,
    resolve_and_install_config_deps::resolve_options,
};
use pnpm_lockfile::{EnvLockfile, PackageKey, PkgName, PkgVerPeer, SnapshotDepRef, SnapshotEntry};
use pnpm_resolving_resolver_base::{ResolveResult, Resolver, WantedDependency};
use std::collections::HashMap;

/// Resolve `parent_manifest.optionalDependencies` and record each into
/// `env_lockfile`'s `packages` + `snapshots`. Returns the
/// `optionalDependencies` map (alias → version) for the parent's
/// snapshot, or `None` when the parent declares none.
pub async fn resolve_optional_subdeps(
    parent_name: &str,
    parent_manifest: &serde_json::Value,
    resolver: &dyn Resolver,
    opts: &ConfigDepsInstallOptions<'_>,
    env_lockfile: &mut EnvLockfile,
) -> Result<Option<HashMap<PkgName, SnapshotDepRef>>, ConfigDepError> {
    let Some(optional_deps) =
        parent_manifest.get("optionalDependencies").and_then(|value| value.as_object())
    else {
        return Ok(None);
    };
    if optional_deps.is_empty() {
        return Ok(None);
    }

    let mut resolved: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    for (subdep_name, subdep_spec) in optional_deps {
        let subdep_spec = subdep_spec.as_str().unwrap_or_default();
        if subdep_spec.parse::<node_semver::Version>().is_err() {
            return Err(ConfigDepError::OptionalNotExact {
                parent_name: parent_name.to_string(),
                subdep_name: subdep_name.clone(),
                spec: subdep_spec.to_string(),
            });
        }

        let (subdep_version, result) =
            resolve_subdep(resolver, opts, parent_name, subdep_name, subdep_spec).await?;
        record_optional_subdep(env_lockfile, opts, subdep_name, &subdep_version, &result)?;

        let ver_peer =
            subdep_version.parse::<PkgVerPeer>().map_err(|_| ConfigDepError::BadConfigDep {
                message: format!(
                    "Resolved optionalDependency version {subdep_version} is not a valid version",
                ),
            })?;
        let pkg_name: PkgName = subdep_name.parse().map_err(|_| ConfigDepError::BadConfigDep {
            message: format!("Resolved optionalDependency name {subdep_name} is invalid"),
        })?;
        resolved.insert(pkg_name, SnapshotDepRef::Plain(ver_peer));
    }

    Ok((!resolved.is_empty()).then_some(resolved))
}

fn record_optional_subdep(
    env_lockfile: &mut EnvLockfile,
    opts: &ConfigDepsInstallOptions<'_>,
    subdep_name: &str,
    subdep_version: &str,
    result: &ResolveResult,
) -> Result<(), ConfigDepError> {
    let registry = opts.pick_registry(subdep_name);
    let pkg_key: PackageKey = format!("{subdep_name}@{subdep_version}").parse().map_err(|_| {
        ConfigDepError::BadConfigDep {
            message: format!(
                "Resolved optionalDependency {subdep_name}@{subdep_version} has an unparsable key",
            ),
        }
    })?;

    env_lockfile.packages.insert(
        pkg_key.clone(),
        package_metadata(subdep_name, subdep_version, result, registry, false)
            .map_err(ConfigDepError::LockfileForm)?,
    );
    env_lockfile
        .snapshots
        .entry(pkg_key)
        .or_insert_with(|| SnapshotEntry { optional: true, ..SnapshotEntry::default() });
    Ok(())
}

/// Resolve one optional subdependency to its version and result, both
/// backed by an integrity.
async fn resolve_subdep(
    resolver: &dyn Resolver,
    opts: &ConfigDepsInstallOptions<'_>,
    parent_name: &str,
    subdep_name: &str,
    subdep_spec: &str,
) -> Result<(String, ResolveResult), ConfigDepError> {
    let no_integrity = || ConfigDepError::BadConfigDep {
        message: format!(
            r#"Cannot resolve optionalDependency "{subdep_name}" of config dependency "{parent_name}" because it has no integrity"#,
        ),
    };
    let wanted = WantedDependency {
        alias: Some(subdep_name.to_string()),
        bare_specifier: Some(subdep_spec.to_string()),
        optional: Some(true),
        ..WantedDependency::default()
    };
    let result = resolver
        .resolve(&wanted, &resolve_options(opts.root_dir))
        .await
        .map_err(|error| ConfigDepError::Resolve {
            spec: format!("{subdep_name}@{subdep_spec}"),
            error,
        })?
        .ok_or_else(no_integrity)?;
    let version = result.name_ver.as_ref().ok_or_else(no_integrity)?.suffix.to_string();
    if !resolution_has_integrity(&result.resolution) {
        return Err(no_integrity());
    }
    Ok((version, result))
}

pub(crate) fn resolution_has_integrity(resolution: &pnpm_lockfile::LockfileResolution) -> bool {
    use pnpm_lockfile::LockfileResolution;
    match resolution {
        LockfileResolution::Registry(_) => true,
        LockfileResolution::Tarball(tarball) => tarball.integrity.is_some(),
        _ => false,
    }
}
