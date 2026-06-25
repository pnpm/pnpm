//! Port of pnpm's
//! [`scanGlobalPackages`](https://github.com/pnpm/pnpm/blob/1819226b51/global/packages/src/scanGlobalPackages.ts):
//! enumerate the package groups installed under the global packages
//! directory and the details needed to list, update, and remove them.

use crate::read_package_json;
use pacquet_cmd_shim::{Host, PackageBinSource, get_bins_from_package_manifest};
use pacquet_resolving_deps_resolver::is_valid_dependency_alias;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

/// A single global install group: a hash symlink pointing at an install
/// directory whose `package.json` lists one or more direct dependencies.
#[derive(Debug, Clone)]
pub struct GlobalPackageInfo {
    pub hash: String,
    pub install_dir: PathBuf,
    /// The `(alias, spec)` pairs from the install dir's `package.json`
    /// `dependencies`.
    pub dependencies: Vec<(String, String)>,
}

impl GlobalPackageInfo {
    /// Whether `alias` is one of this group's direct dependencies.
    #[must_use]
    pub fn has_alias(&self, alias: &str) -> bool {
        self.dependencies.iter().any(|(name, _)| name == alias)
    }

    /// The direct-dependency aliases of this group.
    #[must_use]
    pub fn aliases(&self) -> Vec<String> {
        self.dependencies.iter().map(|(name, _)| name.clone()).collect()
    }
}

/// One installed dependency of a group, with its resolved version and
/// parsed manifest. Mirrors pnpm's `InstalledGlobalPackage`.
#[derive(Debug, Clone)]
pub struct InstalledGlobalPackage {
    pub alias: String,
    pub version: String,
    pub manifest: Value,
}

/// Scan `global_dir` for installed package groups. A missing directory
/// yields an empty list (matching pnpm's ENOENT handling).
#[must_use]
pub fn scan_global_packages(global_dir: &Path) -> Vec<GlobalPackageInfo> {
    let Ok(entries) = std::fs::read_dir(global_dir) else { return Vec::new() };
    let mut result = Vec::new();
    for entry in entries.flatten() {
        // Hash entries are symlinks pointing to install dirs.
        let Ok(file_type) = entry.file_type() else { continue };
        if !file_type.is_symlink() {
            continue;
        }
        let link_path = entry.path();
        let Ok(install_dir) = std::fs::canonicalize(&link_path) else { continue };
        let Some(manifest) = read_package_json(&install_dir) else { continue };
        let dependencies = dependencies_of(&manifest);
        if dependencies.is_empty() {
            continue;
        }
        result.push(GlobalPackageInfo {
            hash: entry.file_name().to_string_lossy().into_owned(),
            install_dir,
            dependencies,
        });
    }
    result
}

/// Find the group that contains `alias`, if any. Mirrors pnpm's
/// `findGlobalPackage`.
#[must_use]
pub fn find_global_package(global_dir: &Path, alias: &str) -> Option<GlobalPackageInfo> {
    scan_global_packages(global_dir).into_iter().find(|pkg| pkg.has_alias(alias))
}

/// Read the installed details (alias, version, manifest) for every direct
/// dependency of `info`. Mirrors pnpm's `getGlobalPackageDetails`.
#[must_use]
pub fn get_global_package_details(info: &GlobalPackageInfo) -> Vec<InstalledGlobalPackage> {
    let modules_dir = info.install_dir.join("node_modules");
    info.dependencies
        .iter()
        .filter_map(|(alias, _)| {
            let manifest = read_package_json(&modules_dir.join(alias))?;
            let version =
                manifest.get("version").and_then(Value::as_str).unwrap_or_default().to_string();
            Some(InstalledGlobalPackage { alias: alias.clone(), version, manifest })
        })
        .collect()
}

/// The bin names installed by a group (deduplicated). Mirrors pnpm's
/// `getInstalledBinNames`.
#[must_use]
pub fn get_installed_bin_names(info: &GlobalPackageInfo) -> Vec<String> {
    let modules_dir = info.install_dir.join("node_modules");
    let mut bins = BTreeSet::new();
    for (alias, _) in &info.dependencies {
        let dep_dir = modules_dir.join(alias);
        let Some(manifest) = read_package_json(&dep_dir) else { continue };
        for command in get_bins_from_package_manifest::<Host>(&manifest, &dep_dir) {
            bins.insert(command.name);
        }
    }
    bins.into_iter().collect()
}

/// Read the directly-installed packages of an install directory as
/// [`PackageBinSource`]s for bin linking / conflict checks. Mirrors
/// pnpm's `readInstalledPackages`.
#[must_use]
pub fn read_installed_packages(install_dir: &Path) -> Vec<PackageBinSource> {
    let Some(manifest) = read_package_json(install_dir) else { return Vec::new() };
    let modules_dir = install_dir.join("node_modules");
    dependencies_of(&manifest)
        .into_iter()
        .filter_map(|(alias, _)| {
            let location = modules_dir.join(&alias);
            let dep_manifest = read_package_json(&location)?;
            Some(PackageBinSource::new(location, Arc::new(dep_manifest)))
        })
        .collect()
}

/// Remove install directories under `global_dir` that no hash symlink
/// points at. Mirrors pnpm's `cleanOrphanedInstallDirs`, including the
/// 5-minute safety window that avoids racing a concurrent install which
/// has created its dir but not yet its symlink.
pub fn clean_orphaned_install_dirs(global_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(global_dir) else { return };
    let entries: Vec<_> = entries.flatten().collect();

    let mut referenced = BTreeSet::new();
    for entry in &entries {
        let Ok(file_type) = entry.file_type() else { continue };
        if !file_type.is_symlink() {
            continue;
        }
        if let Ok(real) = std::fs::canonicalize(entry.path()) {
            referenced.insert(real);
        }
    }

    const SAFETY_WINDOW: Duration = Duration::from_mins(5);
    let now = SystemTime::now();
    for entry in &entries {
        let Ok(file_type) = entry.file_type() else { continue };
        if !file_type.is_dir() {
            continue;
        }
        let dir_path = entry.path();
        let Ok(canonical) = std::fs::canonicalize(&dir_path) else { continue };
        if referenced.contains(&canonical) {
            continue;
        }
        if recently_created(&dir_path, now, SAFETY_WINDOW) {
            continue;
        }
        let _ = std::fs::remove_dir_all(&dir_path);
    }
}

fn recently_created(dir_path: &Path, now: SystemTime, window: Duration) -> bool {
    let Ok(metadata) = std::fs::metadata(dir_path) else { return true };
    // pnpm uses max(birthtime, ctime); std exposes created()/modified(),
    // the closest portable proxies.
    let created = metadata.created().ok();
    let modified = metadata.modified().ok();
    let newest = [created, modified].into_iter().flatten().max();
    match newest {
        Some(time) => now.duration_since(time).map_or(true, |age| age < window),
        None => true,
    }
}

/// Read the `dependencies` map of a global group manifest as `(alias, spec)`
/// pairs, dropping any alias that isn't a valid npm package name.
///
/// The aliases become directory names under `node_modules` at every
/// downstream join site (list, conflict-check, remove, update), so a
/// tampered group `package.json` could otherwise use an alias like `../x`
/// or an absolute path to escape the install directory. Validating here —
/// the single point where aliases enter the scan — closes that for every
/// consumer, using the same [`is_valid_dependency_alias`] check the
/// resolver applies to direct dependencies.
fn dependencies_of(manifest: &Value) -> Vec<(String, String)> {
    manifest
        .get("dependencies")
        .and_then(Value::as_object)
        .map(|deps| {
            deps.iter()
                .filter(|(alias, _)| is_valid_dependency_alias(alias))
                .map(|(alias, spec)| (alias.clone(), spec.as_str().unwrap_or_default().to_string()))
                .collect()
        })
        .unwrap_or_default()
}
