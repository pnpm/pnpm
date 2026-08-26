//! Resolve any config dependencies missing from the env lockfile, then
//! install them all.
//!
//! Handles three input shapes:
//! 1. old object form `{ tarball?, integrity }` — migrated inline into
//!    the lockfile when it carries a tarball URL, otherwise resolved,
//! 2. old string form `<version>+<integrity>` — resolved against the
//!    registry for its tarball URL, keeping the inline integrity,
//! 3. new clean specifier (`1.2.0` / `^1.0.0`) — resolved against the
//!    registry when it isn't already pinned in the lockfile.

use crate::{
    ConfigDepError,
    install_config_deps::install_config_deps,
    options::ConfigDepsInstallOptions,
    parse_integrity::parse_integrity,
    prune::prune_env_lockfile,
    resolve_optional_subdeps::resolve_optional_subdeps,
    verify_env_lockfile::{assert_valid_migrated_config_dep, write_verified_env_lockfile},
};
use pnpm_lockfile::{
    EnvLockfile, LockfileFormOptions, LockfileResolution, PackageKey, PackageMetadata,
    SnapshotEntry, SpecifierAndResolution, TarballResolution,
};
use pnpm_reporter::Reporter;
use pnpm_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};
use pnpm_workspace_state::ConfigDependency;
use ssri::Integrity;
use std::collections::BTreeMap;

/// Config deps keep the npm tarball layout: `registries` is a workspace
/// setting, and config deps are resolved before workspace settings apply. The
/// writer and the reader here agree because both use this same default.
fn npm_lockfile_form(registry: &str) -> LockfileFormOptions<'_> {
    LockfileFormOptions { registry, server_type: None, include_tarball_url: false }
}

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

    let mut to_resolve: Vec<(String, String, Option<Integrity>)> = Vec::new();
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
                    assert_valid_migrated_config_dep(name, &version)?;
                    match detail.tarball.clone() {
                        Some(tarball) => {
                            let registry = opts.pick_registry(name);
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
                        None => to_resolve.push((name.clone(), version, Some(integrity))),
                    }
                }
            }
            ConfigDependency::VersionWithIntegrity(value) if value.contains('+') => {
                if !has_config_dep(&env_lockfile, name) {
                    let (version, integrity) = parse_integrity(name, value)?;
                    assert_valid_migrated_config_dep(name, &version)?;
                    to_resolve.push((name.clone(), version, Some(integrity)));
                }
            }
            ConfigDependency::VersionWithIntegrity(specifier) => {
                if let Some(existing) = config_dep(&env_lockfile, name)
                    && existing.specifier == *specifier
                    && env_lockfile.packages.contains_key(&pkg_key(name, &existing.version)?)
                {
                    continue;
                }
                to_resolve.push((name.clone(), specifier.clone(), None));
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

    for (name, specifier, pinned_integrity) in &to_resolve {
        resolve_one(&mut env_lockfile, resolver, opts, name, specifier, pinned_integrity.as_ref())
            .await?;
    }

    prune_env_lockfile(&mut env_lockfile);
    write_verified_env_lockfile(&env_lockfile, opts.root_dir)?;
    install_config_deps::<Reporter>(&env_lockfile, opts).await
}

/// Resolve a single config dependency and record it (plus one level of
/// optional subdeps) into the env lockfile.
async fn resolve_one(
    env_lockfile: &mut EnvLockfile,
    resolver: &dyn Resolver,
    opts: &ConfigDepsInstallOptions<'_>,
    name: &str,
    specifier: &str,
    pinned_integrity: Option<&Integrity>,
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
    let mut resolution = result.resolution;
    // A migrated dependency keeps the integrity pinned in pnpm-workspace.yaml,
    // so the registry hands over the tarball URL without loosening the pin.
    if let (Some(pinned), LockfileResolution::Tarball(tarball)) =
        (pinned_integrity, &mut resolution)
    {
        tarball.integrity = Some(pinned.clone());
    }
    env_lockfile.packages.insert(
        key.clone(),
        registry_package_metadata(
            resolution
                .to_lockfile_form(name, &version, npm_lockfile_form(registry))
                .map_err(ConfigDepError::LockfileForm)?,
        ),
    );

    // A pinned dependency covers only itself, so its optional subdeps stay out
    // of the lockfile until it is declared as a clean specifier.
    let optional_subdeps = match (pinned_integrity, result.manifest.as_deref()) {
        (None, Some(manifest)) => {
            resolve_optional_subdeps(name, manifest, resolver, opts, env_lockfile).await?
        }
        _ => None,
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
        revision: None,
        git_hosted: None,
        path: None,
    })
    .to_lockfile_form(name, version, npm_lockfile_form(registry))
    .map_err(ConfigDepError::LockfileForm)?;
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
