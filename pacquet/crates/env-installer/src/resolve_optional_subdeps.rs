//! Resolve one level of a config dependency's `optionalDependencies`
//! into the env lockfile. Mirrors pnpm's
//! [`resolveOptionalSubdeps`](https://github.com/pnpm/pnpm/blob/31858c544b/installing/env-installer/src/resolveOptionalSubdeps.ts).
//!
//! Only exact versions are accepted: a range or tag would let the
//! resolved version drift between machines even with a stable parent
//! integrity, breaking the lockfile's reproducibility promise.

use crate::{
    ConfigDepError, manifest_lockfile::package_metadata, options::ConfigDepsInstallOptions,
};
use pacquet_lockfile::{
    EnvLockfile, PackageKey, PkgName, PkgVerPeer, SnapshotDepRef, SnapshotEntry,
};
use pacquet_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};
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

        let wanted = WantedDependency {
            alias: Some(subdep_name.clone()),
            bare_specifier: Some(subdep_spec.to_string()),
            optional: Some(true),
            ..WantedDependency::default()
        };
        let resolve_opts = ResolveOptions {
            project_dir: opts.root_dir.to_path_buf(),
            lockfile_dir: opts.root_dir.to_path_buf(),
            ..ResolveOptions::default()
        };
        let result = resolver
            .resolve(&wanted, &resolve_opts)
            .await
            .map_err(|error| ConfigDepError::Resolve {
                spec: format!("{subdep_name}@{subdep_spec}"),
                error,
            })?
            .ok_or_else(|| ConfigDepError::BadConfigDep {
                message: format!(
                    r#"Cannot resolve optionalDependency "{subdep_name}" of config dependency "{parent_name}" because it has no integrity"#,
                ),
            })?;

        let Some(name_ver) = result.name_ver.as_ref() else {
            return Err(ConfigDepError::BadConfigDep {
                message: format!(
                    r#"Cannot resolve optionalDependency "{subdep_name}" of config dependency "{parent_name}" because it has no integrity"#,
                ),
            });
        };
        if !resolution_has_integrity(&result.resolution) {
            return Err(ConfigDepError::BadConfigDep {
                message: format!(
                    r#"Cannot resolve optionalDependency "{subdep_name}" of config dependency "{parent_name}" because it has no integrity"#,
                ),
            });
        }
        let subdep_version = name_ver.suffix.to_string();
        let registry = opts.pick_registry(subdep_name);
        let pkg_key: PackageKey = format!("{subdep_name}@{subdep_version}")
            .parse()
            .map_err(|_| ConfigDepError::BadConfigDep {
                message: format!("Resolved optionalDependency {subdep_name}@{subdep_version} has an unparsable key"),
            })?;

        env_lockfile.packages.insert(
            pkg_key.clone(),
            package_metadata(subdep_name, &subdep_version, &result, registry, false),
        );
        env_lockfile
            .snapshots
            .entry(pkg_key)
            .or_insert_with(|| SnapshotEntry { optional: true, ..SnapshotEntry::default() });

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

pub(crate) fn resolution_has_integrity(resolution: &pacquet_lockfile::LockfileResolution) -> bool {
    use pacquet_lockfile::LockfileResolution;
    match resolution {
        LockfileResolution::Registry(_) => true,
        LockfileResolution::Tarball(tarball) => tarball.integrity.is_some(),
        _ => false,
    }
}
