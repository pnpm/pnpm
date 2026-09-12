//! Materialize configurational dependencies into
//! `node_modules/.pnpm-config/<name>`.
//!
//! Each config dependency is fetched into the global virtual store
//! (`<store>/links/<gvs-path>/node_modules/<name>`) and symlinked from
//! `.pnpm-config`. Platform-specific optional subdeps are installed one
//! level deep as siblings inside the parent's leaf `node_modules`.

use crate::{
    ConfigDepError, NormalizedConfigDep, NormalizedSubdep, options::ConfigDepsInstallOptions,
    verify_env_lockfile::verify_env_lockfile,
};
use pnpm_graph_hasher::{
    calc_global_virtual_store_path_with_subdeps, calc_leaf_global_virtual_store_path,
    join_global_virtual_store_path,
};
use pnpm_lockfile::{EnvLockfile, LockfileResolution, TarballUrlOptions, npm_tarball_url};
use pnpm_package_is_installable::{
    InstallabilityOptions, PackageInstallabilityManifest, check_package,
};
use pnpm_package_manager::{ImportIndexedDirOpts, import_indexed_dir};
use pnpm_reporter::{
    InstalledConfigDep, InstallingConfigDepsLog, InstallingConfigDepsStatus, LogEvent, LogLevel,
    Reporter, SkippedOptionalDependencyLog, SkippedOptionalPackage, SkippedOptionalReason,
};
use pnpm_store_dir::SharedVerifiedFilesCache;
use pnpm_tarball::IngestTarballToStore;
use ssri::Integrity;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicU8,
};

/// Install every config dependency described by `env_lockfile` and
/// prune any `.pnpm-config` entry that is no longer present.
pub async fn install_config_deps<Reporter: self::Reporter>(
    env_lockfile: &EnvLockfile,
    opts: &ConfigDepsInstallOptions<'_>,
) -> Result<(), ConfigDepError> {
    verify_env_lockfile(env_lockfile)?;
    let normalized = normalize_from_lockfile(env_lockfile, opts)?;
    let global_virtual_store_dir = opts.store_dir.links();
    let config_modules_dir = opts.root_dir.join("node_modules").join(".pnpm-config");

    let existing: Vec<String> = read_dir_names(&config_modules_dir)?;

    let mut started = StartedGate::new();
    prune_removed::<Reporter>(&existing, &normalized, &config_modules_dir, &mut started);

    let logged_methods = AtomicU8::new(0);
    let mut installed: Vec<InstalledConfigDep> = Vec::new();

    for (name, dep) in &normalized {
        let paths = config_dep_paths(name, dep, &config_modules_dir, &global_virtual_store_dir);
        let parent_symlink_already_correct = existing.iter().any(|entry| entry == name)
            && symlink_points_to(&paths.config_dep_path, &paths.pkg_dir_in_gvs);

        materialize_config_dep::<Reporter>(
            opts,
            &logged_methods,
            &mut started,
            name,
            dep,
            &paths,
            &global_virtual_store_dir,
        )
        .await?;

        if parent_symlink_already_correct {
            continue;
        }
        started.report::<Reporter>();
        force_symlink(&paths.pkg_dir_in_gvs, &paths.config_dep_path)?;
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

async fn materialize_config_dep<Reporter: self::Reporter>(
    opts: &ConfigDepsInstallOptions<'_>,
    logged_methods: &AtomicU8,
    started: &mut StartedGate,
    name: &str,
    dep: &NormalizedConfigDep,
    paths: &ConfigDepPaths,
    global_virtual_store_dir: &Path,
) -> Result<(), ConfigDepError> {
    if !paths.pkg_dir_in_gvs.join("package.json").exists() {
        started.report::<Reporter>();
        materialize::<Reporter>(
            opts,
            logged_methods,
            name,
            &dep.version,
            &dep.integrity,
            &dep.tarball,
            &paths.pkg_dir_in_gvs,
        )
        .await?;
    }

    if !dep.optional_subdeps.is_empty() {
        install_optional_subdeps::<Reporter>(
            opts,
            logged_methods,
            started,
            name,
            &dep.version,
            &dep.optional_subdeps,
            global_virtual_store_dir,
            &paths.leaf_node_modules,
        )
        .await?;
    }
    Ok(())
}

/// Drop the `.pnpm-config` links of config dependencies the lockfile no
/// longer lists.
fn prune_removed<Reporter: self::Reporter>(
    existing: &[String],
    normalized: &BTreeMap<String, NormalizedConfigDep>,
    config_modules_dir: &Path,
    started: &mut StartedGate,
) {
    for name in existing {
        if !normalized.contains_key(name) {
            started.report::<Reporter>();
            prune_link(&config_modules_dir.join(name));
        }
    }
}

/// Where one config dependency's package lives in the global virtual store,
/// and where the `.pnpm-config` link to it goes.
struct ConfigDepPaths {
    config_dep_path: PathBuf,
    leaf_node_modules: PathBuf,
    pkg_dir_in_gvs: PathBuf,
}

fn config_dep_paths(
    name: &str,
    dep: &NormalizedConfigDep,
    config_modules_dir: &Path,
    global_virtual_store_dir: &Path,
) -> ConfigDepPaths {
    let parent_full_pkg_id = full_pkg_id(name, &dep.version, &dep.integrity);
    let subdep_ids: BTreeMap<String, String> = dep
        .optional_subdeps
        .iter()
        .map(|subdep| {
            (subdep.name.clone(), full_pkg_id(&subdep.name, &subdep.version, &subdep.integrity))
        })
        .collect();
    let rel_path = calc_global_virtual_store_path_with_subdeps(
        &parent_full_pkg_id,
        name,
        &dep.version,
        &subdep_ids,
    );
    let leaf_node_modules =
        join_global_virtual_store_path(global_virtual_store_dir, &rel_path).join("node_modules");
    ConfigDepPaths {
        config_dep_path: config_modules_dir.join(name),
        pkg_dir_in_gvs: leaf_node_modules.join(name),
        leaf_node_modules,
    }
}

/// Point `link_path` at `target`, creating the link's parent directory.
fn force_symlink(target: &Path, link_path: &Path) -> Result<(), ConfigDepError> {
    if let Some(parent) = link_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| ConfigDepError::Symlink { path: link_path.to_path_buf(), error })?;
    }
    pnpm_fs::force_symlink_dir(target, link_path)
        .map(|_| ())
        .map_err(|error| ConfigDepError::Symlink { path: link_path.to_path_buf(), error })
}

/// Lazily emits the single `pnpm:installing-config-deps started` event,
/// so an install that finds everything already in place stays silent.
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

/// `<name>@<version>:<integrity>` — the package-id shape the GVS hash
/// incorporates.
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
    let cas_paths = IngestTarballToStore {
        http_client: opts.http_client,
        store_dir: opts.store_dir,
        store_index: None,
        store_index_writer: None,
        verify_store_integrity: opts.verify_store_integrity,
        strict_store_pkg_content_check: opts.strict_store_pkg_content_check,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        package_integrity: Some(integrity),
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
        store_projection: pnpm_tarball::ArchiveStoreProjection::Package { append_manifest: None },
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

/// Remove sibling links that no longer belong: the parent's own directory
/// plus every compatible subdep are the only expected entries.
fn prune_unexpected_siblings<Reporter: self::Reporter>(
    parent_name: &str,
    compatible: &[&NormalizedSubdep],
    parent_node_modules_dir: &Path,
    started: &mut StartedGate,
) -> Result<(), ConfigDepError> {
    let mut expected: HashSet<&str> = HashSet::new();
    expected.insert(parent_name);
    for subdep in compatible {
        expected.insert(&subdep.name);
    }
    for sibling in read_dir_names(parent_node_modules_dir)? {
        if !expected.contains(sibling.as_str()) {
            started.report::<Reporter>();
            prune_link(&parent_node_modules_dir.join(&sibling));
        }
    }
    Ok(())
}

/// Remove a stale `.pnpm-config` entry (or optional-subdep sibling).
/// These are directory symlinks/junctions created by
/// [`pnpm_fs::force_symlink_dir`], so they're unlinked via
/// [`pnpm_fs::remove_symlink_dir`] rather than recursively deleted —
/// `remove_dir_all` is the wrong primitive for a link and behaves
/// inconsistently across platforms. A real directory left by an older
/// layout falls back to a recursive remove. A genuine failure (anything
/// but "already gone") is logged rather than silently swallowed.
fn prune_link(path: &Path) {
    let is_link = fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink());
    let result = if is_link { pnpm_fs::remove_symlink_dir(path) } else { fs::remove_dir_all(path) };
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

/// List the immediate child names of `dir`, returning an empty list
/// when the directory is absent.
fn read_dir_names(dir: &Path) -> Result<Vec<String>, ConfigDepError> {
    let mut names = Vec::new();
    for name in dir_entry_names(dir)? {
        // Skip dot-dirs (`.bin`, `.pnpm`, etc.).
        if name.starts_with('.') {
            continue;
        }
        // A scope dir holds the actual `@scope/<pkg>` entries one level
        // down; expand it so the returned names match the scoped package
        // keys callers compare against.
        if name.starts_with('@') {
            names.extend(
                dir_entry_names(&dir.join(&name))?
                    .into_iter()
                    .filter(|child| !child.starts_with('.'))
                    .map(|child| format!("{name}/{child}")),
            );
            continue;
        }
        names.push(name);
    }
    Ok(names)
}

/// The immediate entry names of `dir`, or nothing when the directory does
/// not exist. Names that are not valid UTF-8 cannot be package names, so
/// they are dropped.
fn dir_entry_names(dir: &Path) -> Result<Vec<String>, ConfigDepError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(ConfigDepError::ReadConfigModules { path: dir.to_path_buf(), error });
        }
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| ConfigDepError::ReadConfigModules {
            path: dir.to_path_buf(),
            error,
        })?;
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

/// Whether the symlink (or directory) at `link_path` already resolves
/// to `expected`. Realpaths both sides so a store mounted through a
/// symlink, or case-insensitive filesystems, don't produce false
/// negatives.
fn symlink_points_to(link_path: &Path, expected: &Path) -> bool {
    match (fs::canonicalize(link_path), fs::canonicalize(expected)) {
        (Ok(link_real), Ok(expected_real)) => link_real == expected_real,
        _ => false,
    }
}

mod optional_dependencies;
use optional_dependencies::{install_optional_subdeps, normalize_from_lockfile};
