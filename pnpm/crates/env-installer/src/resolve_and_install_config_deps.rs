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
use pnpm_workspace_state::{ConfigDependency, ConfigDependencyDetail};
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
    let mut lockfile_changed = drop_removed_config_deps(&mut env_lockfile, config_deps);

    for (name, value) in config_deps {
        match plan_config_dep(&mut env_lockfile, opts, name, value)? {
            ConfigDepPlan::Satisfied => {}
            ConfigDepPlan::Migrated => lockfile_changed = true,
            ConfigDepPlan::Resolve { specifier, integrity } => {
                to_resolve.push((name.clone(), specifier, integrity));
            }
        }
    }

    if opts.frozen_lockfile && (lockfile_changed || !to_resolve.is_empty()) {
        return Err(ConfigDepError::FrozenLockfileOutdated {
            message: r#"Cannot update configDependencies with "frozen-lockfile" because the lockfile is not up to date"#.to_string(),
        });
    }

    if to_resolve.is_empty() && !lockfile_changed {
        return install_config_deps::<Reporter>(&env_lockfile, opts).await;
    }

    for (name, specifier, pinned_integrity) in &to_resolve {
        resolve_one(&mut env_lockfile, resolver, opts, name, specifier, pinned_integrity.as_ref())
            .await?;
    }

    // Removal, migration and resolution can each orphan packages and
    // snapshots; drop them before writing.
    prune_env_lockfile(&mut env_lockfile);
    write_verified_env_lockfile(&env_lockfile, opts.root_dir)?;
    install_config_deps::<Reporter>(&env_lockfile, opts).await
}

/// Drop env-lockfile entries for config deps that were removed from
/// `pnpm-workspace.yaml`, so they stop being installed and get pruned from
/// `.pnpm-config`. Reports whether anything was dropped.
fn drop_removed_config_deps(
    env_lockfile: &mut EnvLockfile,
    config_deps: &BTreeMap<String, ConfigDependency>,
) -> bool {
    let importer = env_lockfile.root_importer_mut();
    let before = importer.config_dependencies.len();
    importer.config_dependencies.retain(|name, _| config_deps.contains_key(name));
    importer.config_dependencies.len() != before
}

/// What one declared config dependency still needs before it can be
/// installed.
enum ConfigDepPlan {
    /// The env lockfile already describes it.
    Satisfied,
    /// It was migrated into the env lockfile from the declaration's own
    /// integrity and tarball, so the lockfile needs writing.
    Migrated,
    /// It has to be resolved against the registry.
    Resolve { specifier: String, integrity: Option<Integrity> },
}

fn plan_config_dep(
    env_lockfile: &mut EnvLockfile,
    opts: &ConfigDepsInstallOptions<'_>,
    name: &str,
    value: &ConfigDependency,
) -> Result<ConfigDepPlan, ConfigDepError> {
    match value {
        ConfigDependency::Detailed(detail) => plan_detailed(env_lockfile, opts, name, detail),
        ConfigDependency::VersionWithIntegrity(value) if value.contains('+') => {
            plan_pinned(env_lockfile, name, value)
        }
        ConfigDependency::VersionWithIntegrity(specifier) => {
            plan_specifier(env_lockfile, name, specifier)
        }
    }
}

/// A declaration carrying its own tarball URL is recorded without a
/// resolution round trip.
fn plan_detailed(
    env_lockfile: &mut EnvLockfile,
    opts: &ConfigDepsInstallOptions<'_>,
    name: &str,
    detail: &ConfigDependencyDetail,
) -> Result<ConfigDepPlan, ConfigDepError> {
    if has_config_dep(env_lockfile, name) {
        return Ok(ConfigDepPlan::Satisfied);
    }
    let (version, integrity) = parse_integrity(name, &detail.integrity)?;
    assert_valid_migrated_config_dep(name, &version)?;
    let Some(tarball) = detail.tarball.clone() else {
        return Ok(ConfigDepPlan::Resolve { specifier: version, integrity: Some(integrity) });
    };
    let registry = opts.pick_registry(name);
    migrate_into_lockfile(env_lockfile, name, &version, integrity, tarball, registry)?;
    Ok(ConfigDepPlan::Migrated)
}

/// A `<version>+<integrity>` declaration pins the integrity but still needs
/// the tarball URL a resolution provides.
fn plan_pinned(
    env_lockfile: &EnvLockfile,
    name: &str,
    value: &str,
) -> Result<ConfigDepPlan, ConfigDepError> {
    if has_config_dep(env_lockfile, name) {
        return Ok(ConfigDepPlan::Satisfied);
    }
    let (version, integrity) = parse_integrity(name, value)?;
    assert_valid_migrated_config_dep(name, &version)?;
    Ok(ConfigDepPlan::Resolve { specifier: version, integrity: Some(integrity) })
}

/// A bare specifier is satisfied only when the lockfile already resolved
/// that exact specifier and still holds the package it resolved to.
fn plan_specifier(
    env_lockfile: &EnvLockfile,
    name: &str,
    specifier: &str,
) -> Result<ConfigDepPlan, ConfigDepError> {
    if let Some(existing) = config_dep(env_lockfile, name)
        && existing.specifier == *specifier
        && env_lockfile.packages.contains_key(&pkg_key(name, &existing.version)?)
    {
        return Ok(ConfigDepPlan::Satisfied);
    }
    Ok(ConfigDepPlan::Resolve { specifier: specifier.to_string(), integrity: None })
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
    let resolve_opts = resolve_options(opts.root_dir);
    let no_integrity = || missing_config_integrity(name, specifier);
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

    record_config_dependency(
        env_lockfile,
        &key,
        (name, specifier),
        &version,
        result.resolution,
        pinned_integrity,
        registry,
    )?;

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

fn missing_config_integrity(name: &str, specifier: &str) -> ConfigDepError {
    ConfigDepError::BadConfigDep {
        message: format!(
            "Cannot resolve {name}@{specifier} as a configuration dependency because it has no integrity",
        ),
    }
}

fn record_config_dependency(
    env_lockfile: &mut EnvLockfile,
    key: &PackageKey,
    declaration: (&str, &str),
    version: &str,
    mut resolution: LockfileResolution,
    pinned_integrity: Option<&Integrity>,
    registry: &str,
) -> Result<(), ConfigDepError> {
    let (name, specifier) = declaration;
    env_lockfile.root_importer_mut().config_dependencies.insert(
        name.to_string(),
        SpecifierAndResolution { specifier: specifier.to_string(), version: version.to_string() },
    );
    pin_integrity(&mut resolution, pinned_integrity);
    env_lockfile.packages.insert(
        key.clone(),
        registry_package_metadata(
            resolution
                .to_lockfile_form(name, version, npm_lockfile_form(registry))
                .map_err(ConfigDepError::LockfileForm)?,
        ),
    );

    Ok(())
}

/// A migrated dependency keeps the integrity pinned in pnpm-workspace.yaml,
/// so the registry hands over the tarball URL without loosening the pin.
fn pin_integrity(resolution: &mut LockfileResolution, pinned: Option<&Integrity>) {
    if let (Some(pinned), LockfileResolution::Tarball(tarball)) = (pinned, resolution) {
        tarball.integrity = Some(pinned.clone());
    }
}

pub(crate) fn resolve_options(root_dir: &std::path::Path) -> ResolveOptions {
    ResolveOptions {
        project_dir: root_dir.to_path_buf(),
        lockfile_dir: root_dir.to_path_buf(),
        ..ResolveOptions::default()
    }
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
