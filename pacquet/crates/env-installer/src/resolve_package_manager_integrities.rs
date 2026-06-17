use crate::{
    ConfigDepError,
    manifest_lockfile::{package_metadata, read_dependency_map},
    options::ConfigDepsInstallOptions,
    prune::prune_env_lockfile,
    resolve_optional_subdeps::resolution_has_integrity,
    verify_env_lockfile::write_verified_env_lockfile,
};
use pacquet_lockfile::{
    EnvLockfile, PackageKey, PkgName, PkgVerPeer, SnapshotDepRef, SnapshotEntry,
    SpecifierAndResolution,
};
use pacquet_resolving_resolver_base::{ResolveOptions, ResolveResult, Resolver, WantedDependency};
use std::{collections::HashMap, path::PathBuf};

const PACKAGE_MANAGER_DEPS: [&str; 2] = ["pnpm", "@pnpm/exe"];

pub async fn resolve_package_manager_integrities(
    wanted_specifier: &str,
    pnpm_version: &str,
    resolver: &dyn Resolver,
    opts: &ConfigDepsInstallOptions<'_>,
) -> Result<(), ConfigDepError> {
    let mut env_lockfile = EnvLockfile::read(opts.root_dir)
        .map_err(ConfigDepError::ReadLockfile)?
        .unwrap_or_else(EnvLockfile::create);
    if is_package_manager_resolved(&env_lockfile, wanted_specifier, pnpm_version) {
        return Ok(());
    }
    if opts.frozen_lockfile {
        return Err(ConfigDepError::FrozenLockfileOutdated {
            message: r#"Cannot update packageManagerDependencies with "frozen-lockfile" because the lockfile is not up to date"#.to_string(),
        });
    }

    let mut package_manager_dependencies = std::collections::BTreeMap::new();
    let mut resolved = Vec::new();
    for name in PACKAGE_MANAGER_DEPS {
        let package = resolve_dep(name, pnpm_version, false, resolver, opts).await?;
        package_manager_dependencies.insert(
            name.to_string(),
            SpecifierAndResolution {
                specifier: wanted_specifier.to_string(),
                version: package.version.clone(),
            },
        );
        resolved.push(package);
    }

    env_lockfile.root_importer_mut().package_manager_dependencies =
        Some(package_manager_dependencies);

    let mut seen = std::collections::HashSet::new();
    while let Some(package) = resolved.pop() {
        if !seen.insert(package.key.clone()) {
            if !package.optional
                && let Some(snapshot) = env_lockfile.snapshots.get_mut(&package.key)
            {
                snapshot.optional = false;
            }
            continue;
        }
        let registry = opts.pick_registry(&package.name);
        env_lockfile.packages.insert(
            package.key.clone(),
            package_metadata(&package.name, &package.version, &package.result, registry, false),
        );

        let manifest = package.result.manifest.as_deref();
        let mut dependencies = HashMap::new();
        for (alias, specifier) in read_dependency_map(manifest, "dependencies") {
            let child = resolve_dep(&alias, &specifier, false, resolver, opts).await?;
            dependencies.insert(snapshot_dep_name(&alias)?, child.snapshot_ref(&alias)?);
            resolved.push(child);
        }

        let mut optional_dependencies = HashMap::new();
        for (alias, specifier) in read_dependency_map(manifest, "optionalDependencies") {
            let child = resolve_dep(&alias, &specifier, true, resolver, opts).await?;
            optional_dependencies.insert(snapshot_dep_name(&alias)?, child.snapshot_ref(&alias)?);
            resolved.push(child);
        }

        env_lockfile.snapshots.insert(
            package.key,
            SnapshotEntry {
                dependencies: (!dependencies.is_empty()).then_some(dependencies),
                optional_dependencies: (!optional_dependencies.is_empty())
                    .then_some(optional_dependencies),
                optional: package.optional,
                ..SnapshotEntry::default()
            },
        );
    }

    prune_env_lockfile(&mut env_lockfile);
    write_verified_env_lockfile(&env_lockfile, opts.root_dir)
}

#[must_use]
pub fn is_package_manager_resolved(
    env_lockfile: &EnvLockfile,
    wanted_specifier: &str,
    pnpm_version: &str,
) -> bool {
    let Some(pm_deps) = env_lockfile
        .importers
        .get(EnvLockfile::ROOT_IMPORTER_KEY)
        .and_then(|importer| importer.package_manager_dependencies.as_ref())
    else {
        return false;
    };
    pm_deps.len() == PACKAGE_MANAGER_DEPS.len()
        && PACKAGE_MANAGER_DEPS.iter().all(|name| {
            pm_deps.get(*name).is_some_and(|dep| {
                dep.specifier == wanted_specifier
                    && dep.version == pnpm_version
                    && package_manager_entry_exists(env_lockfile, name, &dep.version)
            })
        })
}

fn package_manager_entry_exists(env_lockfile: &EnvLockfile, name: &str, version: &str) -> bool {
    let Ok(key) = format!("{name}@{version}").parse::<PackageKey>() else {
        return false;
    };
    env_lockfile.packages.contains_key(&key) && env_lockfile.snapshots.contains_key(&key)
}

struct EnvPackage {
    name: String,
    version: String,
    key: PackageKey,
    optional: bool,
    result: ResolveResult,
}

impl EnvPackage {
    fn snapshot_ref(&self, alias: &str) -> Result<SnapshotDepRef, ConfigDepError> {
        if alias == self.name {
            let ver_peer =
                self.version.parse::<PkgVerPeer>().map_err(|_| ConfigDepError::BadConfigDep {
                    message: format!(
                        "Resolved package manager dependency version {} is not valid",
                        self.version,
                    ),
                })?;
            Ok(SnapshotDepRef::Plain(ver_peer))
        } else {
            format!("{}@{}", self.name, self.version).parse().map(SnapshotDepRef::Alias).map_err(
                |_| ConfigDepError::BadConfigDep {
                    message: format!(
                        "Resolved package manager dependency {}@{} has an unparsable alias reference",
                        self.name, self.version,
                    ),
                },
            )
        }
    }
}

async fn resolve_dep(
    alias: &str,
    specifier: &str,
    optional: bool,
    resolver: &dyn Resolver,
    opts: &ConfigDepsInstallOptions<'_>,
) -> Result<EnvPackage, ConfigDepError> {
    let wanted = WantedDependency {
        alias: Some(alias.to_string()),
        bare_specifier: Some(specifier.to_string()),
        optional: optional.then_some(true),
        ..WantedDependency::default()
    };
    let resolve_opts = ResolveOptions {
        project_dir: PathBuf::from(opts.root_dir),
        lockfile_dir: PathBuf::from(opts.root_dir),
        ..ResolveOptions::default()
    };
    let result = resolver
        .resolve(&wanted, &resolve_opts)
        .await
        .map_err(|error| ConfigDepError::Resolve { spec: format!("{alias}@{specifier}"), error })?
        .ok_or_else(|| no_integrity(alias, specifier))?;
    if !resolution_has_integrity(&result.resolution) {
        return Err(no_integrity(alias, specifier));
    }
    let name_ver = result.name_ver.as_ref().ok_or_else(|| no_integrity(alias, specifier))?;
    let name = name_ver.name.to_string();
    let version = name_ver.suffix.to_string();
    let key = format!("{name}@{version}").parse::<PackageKey>().map_err(|_| {
        ConfigDepError::BadConfigDep {
            message: format!(
                "Resolved package manager dependency {name}@{version} has an unparsable lockfile key",
            ),
        }
    })?;
    Ok(EnvPackage { name, version, key, optional, result })
}

fn no_integrity(alias: &str, specifier: &str) -> ConfigDepError {
    ConfigDepError::BadConfigDep {
        message: format!(
            "Cannot resolve {alias}@{specifier} as a package manager dependency because it has no integrity",
        ),
    }
}

fn snapshot_dep_name(alias: &str) -> Result<PkgName, ConfigDepError> {
    alias.parse().map_err(|_| ConfigDepError::BadConfigDep {
        message: format!("Resolved package manager dependency name {alias} is invalid"),
    })
}
