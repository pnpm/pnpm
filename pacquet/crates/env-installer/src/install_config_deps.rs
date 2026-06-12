//! Materialize configurational dependencies into
//! `node_modules/.pnpm-config/<name>`.
//!
//! Mirrors pnpm's
//! [`installConfigDeps`](https://github.com/pnpm/pnpm/blob/31858c544b/installing/env-installer/src/installConfigDeps.ts):
//! each config dependency is fetched into the global virtual store
//! (`<store>/links/<gvs-path>/node_modules/<name>`) and symlinked from
//! `.pnpm-config`. Platform-specific optional subdeps are installed one
//! level deep as siblings inside the parent's leaf `node_modules`.

use crate::{
    ConfigDepError, NormalizedConfigDep, NormalizedSubdep, options::ConfigDepsInstallOptions,
};
use pacquet_graph_hasher::{
    calc_global_virtual_store_path_with_subdeps, calc_leaf_global_virtual_store_path,
};
use pacquet_lockfile::{EnvLockfile, LockfileResolution, npm_tarball_url};
use pacquet_package_is_installable::{
    InstallabilityOptions, PackageInstallabilityManifest, check_package,
};
use pacquet_package_manager::{ImportIndexedDirOpts, import_indexed_dir};
use pacquet_reporter::{
    InstalledConfigDep, InstallingConfigDepsLog, InstallingConfigDepsStatus, LogEvent, LogLevel,
    Reporter, SkippedOptionalDependencyLog, SkippedOptionalPackage, SkippedOptionalReason,
};
use pacquet_store_dir::SharedVerifiedFilesCache;
use pacquet_tarball::DownloadTarballToStore;
use ssri::Integrity;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::Path,
    sync::atomic::AtomicU8,
};

/// Install every config dependency described by `env_lockfile` and
/// prune any `.pnpm-config` entry that is no longer present.
pub async fn install_config_deps<Reporter: self::Reporter>(
    env_lockfile: &EnvLockfile,
    opts: &ConfigDepsInstallOptions<'_>,
) -> Result<(), ConfigDepError> {
    let normalized = normalize_from_lockfile(env_lockfile, opts)?;
    let global_virtual_store_dir = opts.store_dir.links();
    let config_modules_dir = opts.root_dir.join("node_modules").join(".pnpm-config");

    let existing: Vec<String> = read_dir_names(&config_modules_dir)?;

    let mut started = StartedGate::new();

    // Drop config deps that are no longer declared.
    for name in &existing {
        if !normalized.contains_key(name) {
            started.report::<Reporter>();
            prune_link(&config_modules_dir.join(name));
        }
    }

    let logged_methods = AtomicU8::new(0);
    let mut installed: Vec<InstalledConfigDep> = Vec::new();

    for (name, dep) in &normalized {
        let config_dep_path = config_modules_dir.join(name);
        let parent_full_pkg_id = full_pkg_id(name, &dep.version, &dep.integrity);
        let mut subdep_ids = BTreeMap::new();
        for subdep in &dep.optional_subdeps {
            subdep_ids.insert(
                subdep.name.clone(),
                full_pkg_id(&subdep.name, &subdep.version, &subdep.integrity),
            );
        }
        let rel_path = calc_global_virtual_store_path_with_subdeps(
            &parent_full_pkg_id,
            name,
            &dep.version,
            &subdep_ids,
        );
        let leaf_node_modules = global_virtual_store_dir.join(&rel_path).join("node_modules");
        let pkg_dir_in_gvs = leaf_node_modules.join(name);

        let parent_symlink_already_correct = existing.iter().any(|entry| entry == name)
            && symlink_points_to(&config_dep_path, &pkg_dir_in_gvs);

        if !pkg_dir_in_gvs.join("package.json").exists() {
            started.report::<Reporter>();
            materialize::<Reporter>(
                opts,
                &logged_methods,
                name,
                &dep.version,
                &dep.integrity,
                &dep.tarball,
                &pkg_dir_in_gvs,
            )
            .await?;
        }

        if !dep.optional_subdeps.is_empty() {
            install_optional_subdeps::<Reporter>(
                opts,
                &logged_methods,
                &mut started,
                name,
                &dep.version,
                &dep.optional_subdeps,
                &global_virtual_store_dir,
                &leaf_node_modules,
            )
            .await?;
        }

        if parent_symlink_already_correct {
            continue;
        }
        started.report::<Reporter>();
        if let Some(parent) = config_dep_path.parent() {
            fs::create_dir_all(parent).map_err(|error| ConfigDepError::Symlink {
                path: config_dep_path.clone(),
                error,
            })?;
        }
        pacquet_fs::force_symlink_dir(&pkg_dir_in_gvs, &config_dep_path)
            .map_err(|error| ConfigDepError::Symlink { path: config_dep_path.clone(), error })?;
        installed.push(InstalledConfigDep { name: name.clone(), version: dep.version.clone() });
    }

    if !installed.is_empty() {
        Reporter::emit(&LogEvent::InstallingConfigDeps(InstallingConfigDepsLog {
            level: LogLevel::Debug,
            status: InstallingConfigDepsStatus::Done,
            deps: installed,
        }));
    }
    Ok(())
}

/// Lazily emits the single `pnpm:installing-config-deps started` event,
/// so an install that finds everything already in place stays silent.
/// Mirrors upstream's `reportStarted` closure.
struct StartedGate {
    emitted: bool,
}

impl StartedGate {
    fn new() -> Self {
        StartedGate { emitted: false }
    }

    fn report<Reporter: self::Reporter>(&mut self) {
        if self.emitted {
            return;
        }
        self.emitted = true;
        Reporter::emit(&LogEvent::InstallingConfigDeps(InstallingConfigDepsLog {
            level: LogLevel::Debug,
            status: InstallingConfigDepsStatus::Started,
            deps: Vec::new(),
        }));
    }
}

/// `<name>@<version>:<integrity>`. Mirrors upstream's `fullPkgId` shape
/// that the GVS hash incorporates.
fn full_pkg_id(name: &str, version: &str, integrity: &Integrity) -> String {
    format!("{name}@{version}:{integrity}")
}

async fn materialize<Reporter: self::Reporter>(
    opts: &ConfigDepsInstallOptions<'_>,
    logged_methods: &AtomicU8,
    name: &str,
    version: &str,
    integrity: &Integrity,
    tarball: &str,
    dir: &Path,
) -> Result<(), ConfigDepError> {
    let package_id = format!("{name}@{version}");
    let cas_paths = DownloadTarballToStore {
        http_client: opts.http_client,
        store_dir: opts.store_dir,
        store_index: None,
        store_index_writer: None,
        verify_store_integrity: opts.verify_store_integrity,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        package_integrity: integrity,
        package_unpacked_size: None,
        package_file_count: None,
        package_url: tarball,
        package_id: &package_id,
        auth_headers: opts.auth_headers,
        requester: &opts.requester(),
        prefetched_cas_paths: None,
        retry_opts: opts.retry_opts,
        ignore_file_pattern: None,
        offline: opts.offline,
        progress_reported: None,
    }
    .run_without_mem_cache::<Reporter>()
    .await
    .map_err(ConfigDepError::DownloadTarball)?;

    import_indexed_dir::<Reporter>(
        logged_methods,
        opts.package_import_method,
        dir,
        &cas_paths,
        ImportIndexedDirOpts::default(),
    )
    .map_err(ConfigDepError::Import)
}

#[expect(clippy::too_many_arguments, reason = "mirrors upstream's installOptionalSubdeps")]
async fn install_optional_subdeps<Reporter: self::Reporter>(
    opts: &ConfigDepsInstallOptions<'_>,
    logged_methods: &AtomicU8,
    started: &mut StartedGate,
    parent_name: &str,
    parent_version: &str,
    subdeps: &[NormalizedSubdep],
    global_virtual_store_dir: &Path,
    parent_node_modules_dir: &Path,
) -> Result<(), ConfigDepError> {
    let mut compatible: Vec<&NormalizedSubdep> = Vec::new();
    for subdep in subdeps {
        if is_compatible::<Reporter>(opts, parent_name, parent_version, subdep) {
            compatible.push(subdep);
        }
    }

    // Remove sibling links that no longer belong (the parent's own dir
    // plus every compatible subdep are the only expected entries).
    let mut expected: HashSet<&str> = HashSet::new();
    expected.insert(parent_name);
    for subdep in &compatible {
        expected.insert(&subdep.name);
    }
    for sibling in read_dir_names(parent_node_modules_dir)? {
        if !expected.contains(sibling.as_str()) {
            started.report::<Reporter>();
            prune_link(&parent_node_modules_dir.join(&sibling));
        }
    }

    for subdep in compatible {
        let subdep_full_id = full_pkg_id(&subdep.name, &subdep.version, &subdep.integrity);
        let subdep_rel =
            calc_leaf_global_virtual_store_path(&subdep_full_id, &subdep.name, &subdep.version);
        let subdep_dir =
            global_virtual_store_dir.join(&subdep_rel).join("node_modules").join(&subdep.name);
        if !subdep_dir.join("package.json").exists() {
            started.report::<Reporter>();
            materialize::<Reporter>(
                opts,
                logged_methods,
                &subdep.name,
                &subdep.version,
                &subdep.integrity,
                &subdep.tarball,
                &subdep_dir,
            )
            .await?;
        }
        let link_path = parent_node_modules_dir.join(&subdep.name);
        if symlink_points_to(&link_path, &subdep_dir) {
            continue;
        }
        started.report::<Reporter>();
        if let Some(parent) = link_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| ConfigDepError::Symlink { path: link_path.clone(), error })?;
        }
        pacquet_fs::force_symlink_dir(&subdep_dir, &link_path)
            .map_err(|error| ConfigDepError::Symlink { path: link_path.clone(), error })?;
    }
    Ok(())
}

/// Whether `subdep` runs on the host. Subdeps with no platform
/// constraints always pass; otherwise [`check_package`] decides, and an
/// incompatible subdep is logged at debug via
/// `pnpm:skipped-optional-dependency`. Mirrors upstream's use of
/// `checkPackage` (rather than `packageIsInstallable`, which would warn
/// loudly on every install because the env lockfile records all
/// platform variants).
fn is_compatible<Reporter: self::Reporter>(
    opts: &ConfigDepsInstallOptions<'_>,
    parent_name: &str,
    parent_version: &str,
    subdep: &NormalizedSubdep,
) -> bool {
    if subdep.os.is_none() && subdep.cpu.is_none() && subdep.libc.is_none() {
        return true;
    }
    let manifest = PackageInstallabilityManifest {
        name: subdep.name.clone(),
        engines: None,
        cpu: subdep.cpu.clone(),
        os: subdep.os.clone(),
        libc: subdep.libc.clone(),
    };
    let id = format!("{}@{}", subdep.name, subdep.version);
    let options = InstallabilityOptions {
        current_node_version: opts.current_node_version,
        current_os: opts.current_os,
        current_cpu: opts.current_cpu,
        current_libc: opts.current_libc,
        supported_architectures: opts.supported_architectures,
        ..InstallabilityOptions::default()
    };
    match check_package(&id, &manifest, &options) {
        Ok(None) => true,
        Ok(Some(error)) => {
            Reporter::emit(&LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
                level: LogLevel::Debug,
                details: Some(error.to_string()),
                package: SkippedOptionalPackage::Installed {
                    id,
                    name: subdep.name.clone(),
                    version: subdep.version.clone(),
                },
                prefix: opts.root_dir.to_string_lossy().into_owned(),
                reason: match error.skip_reason() {
                    pacquet_package_is_installable::SkipReason::UnsupportedEngine => {
                        SkippedOptionalReason::UnsupportedEngine
                    }
                    pacquet_package_is_installable::SkipReason::UnsupportedPlatform => {
                        SkippedOptionalReason::UnsupportedPlatform
                    }
                },
            }));
            let _ = (parent_name, parent_version);
            false
        }
        // An invalid node version on a platform-only subdep is
        // unreachable (engines is `None`); treat the package as
        // installable rather than aborting the whole config-deps pass.
        Err(_) => true,
    }
}

/// Build the install-set view of `env_lockfile.importers["."]`. Mirrors
/// upstream's `normalizeFromLockfile`, surfacing
/// `ENV_LOCKFILE_CORRUPTED` for a `configDependencies` entry whose
/// `packages:` row (or integrity) is missing.
fn normalize_from_lockfile(
    env_lockfile: &EnvLockfile,
    opts: &ConfigDepsInstallOptions<'_>,
) -> Result<BTreeMap<String, NormalizedConfigDep>, ConfigDepError> {
    let mut deps = BTreeMap::new();
    let Some(importer) = env_lockfile.importers.get(EnvLockfile::ROOT_IMPORTER_KEY) else {
        return Ok(deps);
    };
    for (name, spec) in &importer.config_dependencies {
        let pkg_key = format!("{name}@{}", spec.version);
        let key = pkg_key.parse().map_err(|_| ConfigDepError::EnvLockfileCorrupted {
            message: format!(
                "pnpm-lock.yaml has an unparsable config-dependency key \"{pkg_key}\"",
            ),
        })?;
        let pkg = env_lockfile.packages.get(&key).ok_or_else(|| {
            ConfigDepError::EnvLockfileCorrupted {
                message: format!(
                    "pnpm-lock.yaml is corrupted or incomplete: missing packages entry for \
                     \"{pkg_key}\" referenced from importers['.'].configDependencies",
                ),
            }
        })?;
        // Derive the tarball URL (when integrity-only) from the registry
        // that serves this package, honoring per-scope registry entries —
        // matching upstream's `pickRegistryForPackage(registries, name)`.
        let (integrity, tarball) = integrity_and_tarball(
            &pkg.resolution,
            name,
            &spec.version,
            opts.pick_registry(name),
        )
        .ok_or_else(|| ConfigDepError::EnvLockfileCorrupted {
            message: format!(
                "pnpm-lock.yaml is corrupted or incomplete: missing integrity for \"{pkg_key}\"",
            ),
        })?;

        let optional_subdeps = env_lockfile
            .snapshots
            .get(&key)
            .and_then(|snapshot| snapshot.optional_dependencies.as_ref())
            .map(|optionals| read_optional_subdeps(name, optionals, env_lockfile, opts))
            .transpose()?
            .unwrap_or_default();

        deps.insert(
            name.clone(),
            NormalizedConfigDep {
                version: spec.version.clone(),
                integrity,
                tarball,
                optional_subdeps,
            },
        );
    }
    Ok(deps)
}

fn read_optional_subdeps(
    parent_name: &str,
    optionals: &std::collections::HashMap<
        pacquet_lockfile::PkgName,
        pacquet_lockfile::SnapshotDepRef,
    >,
    env_lockfile: &EnvLockfile,
    opts: &ConfigDepsInstallOptions<'_>,
) -> Result<Vec<NormalizedSubdep>, ConfigDepError> {
    let mut subdeps = Vec::new();
    for (subdep_name, dep_ref) in optionals {
        let version = dep_ref.ver_peer().map(std::string::ToString::to_string).unwrap_or_default();
        let subdep_name = subdep_name.to_string();
        let subdep_key = format!("{subdep_name}@{version}");
        let key = subdep_key.parse().map_err(|_| ConfigDepError::EnvLockfileCorrupted {
            message: format!("pnpm-lock.yaml has an unparsable subdep key \"{subdep_key}\""),
        })?;
        let pkg = env_lockfile.packages.get(&key).ok_or_else(|| {
            ConfigDepError::EnvLockfileCorrupted {
                message: format!(
                    "pnpm-lock.yaml is corrupted or incomplete: missing packages entry for \
                     \"{subdep_key}\" referenced from optionalDependencies of config dependency \
                     \"{parent_name}\"",
                ),
            }
        })?;
        let (integrity, tarball) = integrity_and_tarball(
            &pkg.resolution,
            &subdep_name,
            &version,
            opts.pick_registry(&subdep_name),
        )
        .ok_or_else(|| ConfigDepError::EnvLockfileCorrupted {
            message: format!(
                "pnpm-lock.yaml is corrupted or incomplete: missing integrity for \
                         \"{subdep_key}\"",
            ),
        })?;
        subdeps.push(NormalizedSubdep {
            name: subdep_name.clone(),
            version,
            integrity,
            tarball,
            os: pkg.os.clone(),
            cpu: pkg.cpu.clone(),
            libc: pkg.libc.clone(),
        });
    }
    Ok(subdeps)
}

/// Extract `(integrity, tarball_url)` from a lockfile-form resolution,
/// deriving the canonical npm tarball URL when the registry resolution
/// omitted it. Mirrors upstream's `resolution.tarball ?? getNpmTarballUrl(...)`.
fn integrity_and_tarball(
    resolution: &LockfileResolution,
    name: &str,
    version: &str,
    registry: &str,
) -> Option<(Integrity, String)> {
    match resolution {
        LockfileResolution::Registry(registry_resolution) => {
            Some((registry_resolution.integrity.clone(), npm_tarball_url(name, version, registry)))
        }
        LockfileResolution::Tarball(tarball) => {
            let integrity = tarball.integrity.clone()?;
            Some((integrity, tarball.tarball.clone()))
        }
        _ => None,
    }
}

/// Remove a stale `.pnpm-config` entry (or optional-subdep sibling).
/// These are directory symlinks/junctions created by
/// [`pacquet_fs::force_symlink_dir`], so they're unlinked via
/// [`pacquet_fs::remove_symlink_dir`] rather than recursively deleted —
/// `remove_dir_all` is the wrong primitive for a link and behaves
/// inconsistently across platforms. A real directory left by an older
/// layout falls back to a recursive remove. A genuine failure (anything
/// but "already gone") is logged rather than silently swallowed.
fn prune_link(path: &Path) {
    let is_link = fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink());
    let result =
        if is_link { pacquet_fs::remove_symlink_dir(path) } else { fs::remove_dir_all(path) };
    if let Err(error) = result
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(
            target: "pacquet::env_installer",
            ?path,
            %error,
            "failed to prune stale config-dependency link",
        );
    }
}

/// List the immediate child names of `dir`. Mirrors upstream's
/// `readModulesDir`, returning an empty list when the directory is
/// absent.
fn read_dir_names(dir: &Path) -> Result<Vec<String>, ConfigDepError> {
    let mut names = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(names),
        Err(error) => {
            return Err(ConfigDepError::ReadConfigModules { path: dir.to_path_buf(), error });
        }
    };
    for entry in entries {
        let entry = entry.map_err(|error| ConfigDepError::ReadConfigModules {
            path: dir.to_path_buf(),
            error,
        })?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
        // Skip dot-dirs (`.bin`, `.pnpm`, etc.), matching `readModulesDir`.
        if name.starts_with('.') {
            continue;
        }
        // A scope dir holds the actual `@scope/<pkg>` entries one level
        // down; expand it so the returned names match the scoped package
        // keys callers compare against. Mirrors upstream's `readModulesDir`.
        if name.starts_with('@') {
            let scope_dir = dir.join(&name);
            match fs::read_dir(&scope_dir) {
                Ok(children) => {
                    for child in children {
                        let child = child.map_err(|error| ConfigDepError::ReadConfigModules {
                            path: scope_dir.clone(),
                            error,
                        })?;
                        if let Some(child_name) = child.file_name().to_str()
                            && !child_name.starts_with('.')
                        {
                            names.push(format!("{name}/{child_name}"));
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(ConfigDepError::ReadConfigModules { path: scope_dir, error });
                }
            }
            continue;
        }
        names.push(name);
    }
    Ok(names)
}

/// Whether the symlink (or directory) at `link_path` already resolves
/// to `expected`. Realpaths both sides so a store mounted through a
/// symlink, or case-insensitive filesystems, don't produce false
/// negatives. Mirrors upstream's `symlinkPointsTo`.
fn symlink_points_to(link_path: &Path, expected: &Path) -> bool {
    match (fs::canonicalize(link_path), fs::canonicalize(expected)) {
        (Ok(link_real), Ok(expected_real)) => link_real == expected_real,
        _ => false,
    }
}
