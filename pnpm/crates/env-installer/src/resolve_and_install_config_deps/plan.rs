//! Decide what each declared config dependency still needs before it can
//! be installed.

use super::{ConfigDepUpdates, config_dep, has_config_dep, migrate_into_lockfile, pkg_key};
use crate::{
    ConfigDepError, options::ConfigDepsInstallOptions, parse_integrity::parse_integrity,
    verify_env_lockfile::assert_valid_migrated_config_dep,
};
use pnpm_lockfile::EnvLockfile;
use pnpm_workspace_state::{ConfigDependency, ConfigDependencyDetail};
use ssri::Integrity;
use std::collections::BTreeMap;

/// A config dependency to resolve: its name, the specifier to resolve, and
/// the integrity its declaration pins, if any.
pub(super) type ToResolve = (String, String, Option<Integrity>);

/// The config dependencies that need resolving, and whether the env
/// lockfile changed without a resolution, from dropping removed entries or
/// migrating declarations into it.
pub(super) fn plan_config_deps(
    env_lockfile: &mut EnvLockfile,
    config_deps: &BTreeMap<String, ConfigDependency>,
    updates: &ConfigDepUpdates,
    opts: &ConfigDepsInstallOptions<'_>,
) -> Result<(Vec<ToResolve>, bool), ConfigDepError> {
    let mut to_resolve = Vec::new();
    let mut lockfile_changed = drop_removed_config_deps(env_lockfile, config_deps);
    for (name, value) in config_deps {
        let update = updates.prev_specifiers.contains_key(name);
        match plan_config_dep(env_lockfile, opts, name, value, update)? {
            ConfigDepPlan::Satisfied => {}
            ConfigDepPlan::Migrated => lockfile_changed = true,
            ConfigDepPlan::Resolve { specifier, integrity } => {
                to_resolve.push((name.clone(), specifier, integrity));
            }
        }
    }
    Ok((to_resolve, lockfile_changed))
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
    update: bool,
) -> Result<ConfigDepPlan, ConfigDepError> {
    match value {
        ConfigDependency::Detailed(detail) => plan_detailed(env_lockfile, opts, name, detail),
        ConfigDependency::VersionWithIntegrity(value) if value.contains('+') => {
            plan_pinned(env_lockfile, name, value)
        }
        ConfigDependency::VersionWithIntegrity(specifier) if update => {
            Ok(ConfigDepPlan::Resolve { specifier: specifier.clone(), integrity: None })
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
    let registry = opts.verification.pick_registry(name);
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
