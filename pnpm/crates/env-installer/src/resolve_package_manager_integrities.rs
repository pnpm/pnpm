use crate::{
    ConfigDepError,
    manifest_lockfile::{package_metadata, read_dependency_map},
    options::ConfigDepsInstallOptions,
    prune::prune_env_lockfile,
    resolve_optional_subdeps::resolution_has_integrity,
    verify_env_lockfile::{verify_env_lockfile, write_verified_env_lockfile},
};
use pnpm_lockfile::{
    EnvLockfile, LockfileResolution, PackageKey, PkgName, PkgVerPeer, RegistryResolution,
    SnapshotDepRef, SnapshotEntry, SpecifierAndResolution, TarballResolution,
};
use pnpm_resolving_resolver_base::{ResolveOptions, ResolveResult, Resolver, WantedDependency};
use std::{collections::HashMap, path::PathBuf};

const PACKAGE_MANAGER_DEPS_WITH_EXE: [&str; 2] = ["pnpm", "@pnpm/exe"];
const PACKAGE_MANAGER_DEPS_PNPM_ONLY: [&str; 1] = ["pnpm"];
const PNPM_EXE_INTRODUCED: (u64, u64, u64) = (6, 17, 1);

/// Resolve the closure of `package_manager_deps` — the packages a package
/// manager is installed from — at `version`, record it in the env lockfile
/// at `opts.root_dir`, and return that lockfile.
///
/// `force_resync` skips the recorded-entries fast path, so entries that look
/// up to date but are invalid (e.g. resolutions carrying tarball URLs
/// written by an earlier pnpm) are discarded and re-resolved. Such entries
/// already record the version the manifest pins — only pnpm's own reading of
/// them is at stake — so under `opts.frozen_lockfile` the re-resolution
/// happens in memory and the lockfile is left untouched, rather than failing
/// a command the manifest and the lockfile agree on. Entries that do not
/// record the pinned version are the case `--frozen-lockfile` exists for and
/// still fail.
pub async fn resolve_package_manager_integrities(
    package_manager_deps: &[&str],
    wanted_specifier: &str,
    version: &str,
    resolver: &dyn Resolver,
    opts: &ConfigDepsInstallOptions<'_>,
    force_resync: bool,
) -> Result<EnvLockfile, ConfigDepError> {
    let mut env_lockfile = EnvLockfile::read(opts.root_dir)
        .map_err(ConfigDepError::ReadLockfile)?
        .unwrap_or_else(EnvLockfile::create);
    if !force_resync
        && is_package_manager_resolved_with_deps(
            &env_lockfile,
            wanted_specifier,
            version,
            package_manager_deps,
        )
    {
        return Ok(env_lockfile);
    }
    let repair_in_memory = force_resync && opts.frozen_lockfile;
    if opts.frozen_lockfile && !force_resync {
        if pins_wanted_package_manager(&env_lockfile, version, package_manager_deps) {
            return Ok(env_lockfile);
        }
        return Err(ConfigDepError::FrozenLockfileOutdated {
            message: r#"Cannot update packageManagerDependencies with "frozen-lockfile" because the lockfile is not up to date"#.to_string(),
        });
    }

    let mut package_manager_dependencies = std::collections::BTreeMap::new();
    let mut resolved = Vec::new();
    for name in package_manager_deps {
        let package = resolve_dep(name, version, false, resolver, opts).await?;
        package_manager_dependencies.insert(
            (*name).to_string(),
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
        let mut metadata =
            package_metadata(&package.name, &package.version, &package.result, registry, false)
                .map_err(ConfigDepError::LockfileForm)?;
        metadata.resolution = strip_registry_tarball_url(metadata.resolution);
        env_lockfile.packages.insert(package.key.clone(), metadata);

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
    if repair_in_memory {
        verify_env_lockfile(&env_lockfile)?;
    } else {
        write_verified_env_lockfile(&env_lockfile, opts.root_dir)?;
    }
    Ok(env_lockfile)
}

/// Rewrite a registry tarball resolution to integrity-only form, dropping
/// tarball URLs a registry advertises on a host other than its own —
/// load-balanced proxies and Artifactory-style mirrors do this, see
/// <https://github.com/pnpm/pnpm/issues/13619>. The package-manager
/// bootstrap never fetches a URL recorded in the lockfile: the download URL
/// is always derived from the trusted bootstrap registries at install time,
/// so a repository-provided entry cannot steer the download. Dropping the
/// URL here keeps freshly resolved entries in exactly the integrity-only
/// shape the bootstrap validation accepts.
fn strip_registry_tarball_url(resolution: LockfileResolution) -> LockfileResolution {
    match resolution {
        LockfileResolution::Tarball(TarballResolution {
            tarball,
            integrity: Some(integrity),
            revision: None,
            git_hosted: None | Some(false),
            path: None,
        }) if !tarball.starts_with("file:") => {
            LockfileResolution::Registry(RegistryResolution { integrity, revision: None })
        }
        other => other,
    }
}

#[must_use]
pub fn is_package_manager_resolved(
    env_lockfile: &EnvLockfile,
    wanted_specifier: &str,
    pnpm_version: &str,
) -> bool {
    is_package_manager_resolved_with_deps(
        env_lockfile,
        wanted_specifier,
        pnpm_version,
        pnpm_engine_packages(pnpm_version),
    )
}

/// Whether the env lockfile already records what this pnpm would write for
/// `pnpm_version`: the pinned packages under the specifier they were pinned
/// from, and nothing besides them.
fn is_package_manager_resolved_with_deps(
    env_lockfile: &EnvLockfile,
    wanted_specifier: &str,
    pnpm_version: &str,
    package_manager_deps: &[&str],
) -> bool {
    recorded_package_manager_deps(env_lockfile).is_some_and(|pm_deps| {
        pm_deps.len() == package_manager_deps.len()
            && pm_deps.values().all(|dep| dep.specifier == wanted_specifier)
    }) && pins_wanted_package_manager(env_lockfile, pnpm_version, package_manager_deps)
}

/// Whether the env lockfile pins the package manager the manifest asks for,
/// even when it records more packages than this pnpm installs it from.
///
/// A pnpm below 11.20.0 pins `@pnpm/exe` beside `pnpm` for a v12 version,
/// because that is the set its own major is installed from. Such an entry
/// pins the wanted version through the same integrity and cannot change
/// which pnpm runs, so a frozen lockfile accepts it instead of failing a
/// project whose lockfile a teammate's older pnpm last wrote. An entry
/// pinning any other version, or one the lockfile carries no package to
/// install from, is a lockfile that disagrees with the manifest, which is
/// what the flag is for, and a writable install still rewrites the block to
/// the packages this pnpm installs from.
fn pins_wanted_package_manager(
    env_lockfile: &EnvLockfile,
    pnpm_version: &str,
    package_manager_deps: &[&str],
) -> bool {
    let Some(pm_deps) = recorded_package_manager_deps(env_lockfile) else {
        return false;
    };
    package_manager_deps.iter().all(|name| pm_deps.contains_key(*name))
        && pm_deps.iter().all(|(name, dep)| {
            dep.version == pnpm_version
                && package_manager_entry_exists(env_lockfile, name, &dep.version)
        })
}

fn recorded_package_manager_deps(
    env_lockfile: &EnvLockfile,
) -> Option<&std::collections::BTreeMap<String, SpecifierAndResolution>> {
    env_lockfile
        .importers
        .get(EnvLockfile::ROOT_IMPORTER_KEY)
        .and_then(|importer| importer.package_manager_dependencies.as_ref())
}

/// The packages the env lockfile pins for `pnpm_version`.
///
/// `>=6.17.1 <12` publishes the JS `pnpm` and the native `@pnpm/exe` as two
/// packages, and both are pinned, because the pin is shared and teammates
/// may run either one. Every other version publishes `pnpm` alone: as the JS
/// CLI below 6.17.1, and as the native executable itself from 12.
#[must_use]
pub fn pnpm_engine_packages(pnpm_version: &str) -> &'static [&'static str] {
    let Some(version) = node_semver::Version::parse(pnpm_version).ok() else {
        return &PACKAGE_MANAGER_DEPS_WITH_EXE;
    };
    if version.major >= 12 {
        return &PACKAGE_MANAGER_DEPS_PNPM_ONLY;
    }
    if (version.major, version.minor, version.patch) >= PNPM_EXE_INTRODUCED {
        &PACKAGE_MANAGER_DEPS_WITH_EXE
    } else {
        &PACKAGE_MANAGER_DEPS_PNPM_ONLY
    }
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
