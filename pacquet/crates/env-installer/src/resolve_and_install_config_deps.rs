//! Resolve any config dependencies missing from the env lockfile, then
//! install them all. Mirrors pnpm's
//! [`resolveAndInstallConfigDeps`](https://github.com/pnpm/pnpm/blob/31858c544b/installing/env-installer/src/resolveAndInstallConfigDeps.ts).
//!
//! Handles three input shapes, matching upstream:
//! 1. old object form `{ tarball?, integrity }` — migrated inline into
//!    the lockfile,
//! 2. old string form `<version>+<integrity>` — migrated inline,
//! 3. new clean specifier (`1.2.0` / `^1.0.0`) — resolved against the
//!    registry when it isn't already pinned in the lockfile.

use crate::{
    ConfigDepError, install_config_deps::install_config_deps, options::ConfigDepsInstallOptions,
    parse_integrity::parse_integrity, prune::prune_env_lockfile,
    resolve_optional_subdeps::resolve_optional_subdeps,
    verify_env_lockfile::write_verified_env_lockfile,
};
use pacquet_lockfile::{
    EnvLockfile, LockfileResolution, PackageKey, PackageMetadata, SnapshotEntry,
    SpecifierAndResolution, TarballResolution, npm_tarball_url,
};
use pacquet_reporter::Reporter;
use pacquet_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};
use pacquet_workspace_state::ConfigDependency;
use ssri::Integrity;
use std::collections::BTreeMap;

/// Resolve + install the config dependencies declared in
/// `pnpm-workspace.yaml` (`config_deps`).
pub async fn resolve_and_install_config_deps<Reporter: self::Reporter>(
    config_deps: &BTreeMap<String, ConfigDependency>,
    resolver: &dyn Resolver,
    opts: &ConfigDepsInstallOptions<'_>,
) -> Result<(), ConfigDepError> {
    let mut env_lockfile = EnvLockfile::read(opts.root_dir)
        .map_err(ConfigDepError::ReadLockfile)?
        .unwrap_or_else(EnvLockfile::create);

    let mut to_resolve: Vec<(String, String)> = Vec::new();
    let mut lockfile_changed = false;

    // Drop env-lockfile entries for config deps that were removed from
    // `pnpm-workspace.yaml`, so they stop being installed and get pruned
    // from `.pnpm-config`. The packages/snapshots they referenced are
    // cleaned up by `prune_env_lockfile` below.
    {
        let importer = env_lockfile.root_importer_mut();
        let before = importer.config_dependencies.len();
        importer.config_dependencies.retain(|name, _| config_deps.contains_key(name));
        lockfile_changed |= importer.config_dependencies.len() != before;
    }

    for (name, value) in config_deps {
        match value {
            ConfigDependency::Detailed(detail) => {
                if !has_config_dep(&env_lockfile, name) {
                    let (version, integrity) = parse_integrity(name, &detail.integrity)?;
                    let registry = opts.pick_registry(name);
                    let tarball = detail
                        .tarball
                        .clone()
                        .unwrap_or_else(|| npm_tarball_url(name, &version, registry));
                    migrate_into_lockfile(
                        &mut env_lockfile,
                        name,
                        &version,
                        integrity,
                        tarball,
                        registry,
                    )?;
                    lockfile_changed = true;
                }
            }
            ConfigDependency::VersionWithIntegrity(value) if value.contains('+') => {
                if !has_config_dep(&env_lockfile, name) {
                    let (version, integrity) = parse_integrity(name, value)?;
                    let registry = opts.pick_registry(name);
                    let tarball = npm_tarball_url(name, &version, registry);
                    migrate_into_lockfile(
                        &mut env_lockfile,
                        name,
                        &version,
                        integrity,
                        tarball,
                        registry,
                    )?;
                    lockfile_changed = true;
                }
            }
            ConfigDependency::VersionWithIntegrity(specifier) => {
                if let Some(existing) = config_dep(&env_lockfile, name)
                    && existing.specifier == *specifier
                    && env_lockfile.packages.contains_key(&pkg_key(name, &existing.version)?)
                {
                    continue;
                }
                to_resolve.push((name.clone(), specifier.clone()));
            }
        }
    }

    if opts.frozen_lockfile && (lockfile_changed || !to_resolve.is_empty()) {
        return Err(ConfigDepError::FrozenLockfileOutdated {
            message: r#"Cannot update configDependencies with "frozen-lockfile" because the lockfile is not up to date"#.to_string(),
        });
    }

    if to_resolve.is_empty() {
        if lockfile_changed {
            // Migration and/or removal changed the lockfile; prune any
            // now-orphaned packages/snapshots before writing.
            prune_env_lockfile(&mut env_lockfile);
            write_verified_env_lockfile(&env_lockfile, opts.root_dir)?;
        }
        return install_config_deps::<Reporter>(&env_lockfile, opts).await;
    }

    for (name, specifier) in &to_resolve {
        resolve_one(&mut env_lockfile, resolver, opts, name, specifier).await?;
    }

    prune_env_lockfile(&mut env_lockfile);
    write_verified_env_lockfile(&env_lockfile, opts.root_dir)?;
    install_config_deps::<Reporter>(&env_lockfile, opts).await
}

/// Resolve a single clean-specifier config dependency and record it
/// (plus one level of optional subdeps) into the env lockfile.
async fn resolve_one(
    env_lockfile: &mut EnvLockfile,
    resolver: &dyn Resolver,
    opts: &ConfigDepsInstallOptions<'_>,
    name: &str,
    specifier: &str,
) -> Result<(), ConfigDepError> {
    let wanted = WantedDependency {
        alias: Some(name.to_string()),
        bare_specifier: Some(specifier.to_string()),
        ..WantedDependency::default()
    };
    let resolve_opts = ResolveOptions {
        project_dir: opts.root_dir.to_path_buf(),
        lockfile_dir: opts.root_dir.to_path_buf(),
        ..ResolveOptions::default()
    };
    let no_integrity = || ConfigDepError::BadConfigDep {
        message: format!(
            "Cannot resolve {name}@{specifier} as a configuration dependency because it has no integrity",
        ),
    };
    let result = resolver
        .resolve(&wanted, &resolve_opts)
        .await
        .map_err(|error| ConfigDepError::Resolve { spec: format!("{name}@{specifier}"), error })?
        .ok_or_else(no_integrity)?;

    if !crate::resolve_optional_subdeps::resolution_has_integrity(&result.resolution) {
        return Err(no_integrity());
    }
    let version = result.name_ver.as_ref().ok_or_else(no_integrity)?.suffix.to_string();
    let registry = opts.pick_registry(name);
    let key = pkg_key(name, &version)?;

    env_lockfile.root_importer_mut().config_dependencies.insert(
        name.to_string(),
        SpecifierAndResolution { specifier: specifier.to_string(), version: version.clone() },
    );
    env_lockfile.packages.insert(
        key.clone(),
        registry_package_metadata(
            result.resolution.to_lockfile_form(name, &version, registry, false),
        ),
    );

    let optional_subdeps = match result.manifest.as_deref() {
        Some(manifest) => {
            resolve_optional_subdeps(name, manifest, resolver, opts, env_lockfile).await?
        }
        None => None,
    };
    env_lockfile.snapshots.insert(
        key,
        SnapshotEntry { optional_dependencies: optional_subdeps, ..SnapshotEntry::default() },
    );
    Ok(())
}

/// Insert the lockfile entries for an old-format config dependency
/// being migrated inline (object or `version+integrity` string form).
fn migrate_into_lockfile(
    env_lockfile: &mut EnvLockfile,
    name: &str,
    version: &str,
    integrity: Integrity,
    tarball: String,
    registry: &str,
) -> Result<(), ConfigDepError> {
    let key = pkg_key(name, version)?;
    env_lockfile.root_importer_mut().config_dependencies.insert(
        name.to_string(),
        SpecifierAndResolution { specifier: version.to_string(), version: version.to_string() },
    );
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball,
        integrity: Some(integrity),
        git_hosted: None,
        path: None,
    })
    .to_lockfile_form(name, version, registry, false);
    env_lockfile.packages.insert(key.clone(), registry_package_metadata(resolution));
    env_lockfile.snapshots.insert(key, SnapshotEntry::default());
    Ok(())
}

/// A `packages:` entry carrying only a resolution — the shape a config
/// dependency (with no peer/engine metadata of its own) takes.
fn registry_package_metadata(resolution: LockfileResolution) -> PackageMetadata {
    PackageMetadata {
        resolution,
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

fn has_config_dep(env_lockfile: &EnvLockfile, name: &str) -> bool {
    config_dep(env_lockfile, name).is_some()
}

fn config_dep<'a>(env_lockfile: &'a EnvLockfile, name: &str) -> Option<&'a SpecifierAndResolution> {
    env_lockfile.importers.get(EnvLockfile::ROOT_IMPORTER_KEY)?.config_dependencies.get(name)
}

fn pkg_key(name: &str, version: &str) -> Result<PackageKey, ConfigDepError> {
    format!("{name}@{version}").parse().map_err(|_| ConfigDepError::BadConfigDep {
        message: format!("Config dependency {name}@{version} has an unparsable lockfile key"),
    })
}
