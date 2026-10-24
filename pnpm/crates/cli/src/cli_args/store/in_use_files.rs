//! The store files of the packages that registered projects installed.
//!
//! A package imported by copy or clone, such as a patched or built one, has
//! no hard link to the store, so `store prune` needs this list to keep it.

use pnpm_deps_restorer::{binary_store_index_key, store_index_key_for_resolution};
use pnpm_lockfile::{Lockfile, LockfileResolution, PackageKey};
use pnpm_store_dir::{
    CafsFileInfo, PackageFilesIndex, PruneError, StoreDir, StoreIndex, get_registered_projects,
};
use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
};

/// Every file of the store index rows of the packages in each registered
/// project's current lockfile, including the files of every side-effects
/// entry. A project without a current lockfile and a package without a row
/// add nothing. A registry error keeps its own code, and any other
/// failure is [`PruneError::InUseFiles`].
pub(super) fn in_use_files(store_dir: &StoreDir) -> Result<HashSet<PathBuf>, PruneError> {
    if !StoreIndex::exists(store_dir.root()) {
        return Ok(HashSet::new());
    }
    let mut keys = HashSet::new();
    for project in get_registered_projects(store_dir).map_err(PruneError::ListProjects)? {
        if let Some(lockfile) = load_installed_lockfile(&project).map_err(PruneError::InUseFiles)? {
            keys.extend(package_index_keys(&lockfile).map_err(PruneError::InUseFiles)?);
        }
    }
    if keys.is_empty() {
        return Ok(HashSet::new());
    }
    let keys: Vec<String> = keys.into_iter().collect();
    let rows = StoreIndex::open_readonly_in(store_dir)
        .and_then(|index| index.get_many(&keys))
        .map_err(|error| PruneError::InUseFiles(io::Error::other(error)))?;
    Ok(rows
        .values()
        .flat_map(row_files)
        .filter_map(|file| store_dir.cas_file_path_by_mode(&file.digest, file.mode))
        .collect())
}

/// The lockfile the last install wrote into the project's
/// `node_modules`, which records what is installed there.
fn load_installed_lockfile(project: &Path) -> io::Result<Option<Lockfile>> {
    // A project with a custom modules directory has no lockfile here, so
    // only their hard links keep its packages.
    let install_state_dir = project.join("node_modules").join(".pnpm");
    Lockfile::load_current_from_install_state_dir(&install_state_dir)
        .map_err(|error| io::Error::other(format!("{}: {error}", install_state_dir.display())))
}

fn package_index_keys(lockfile: &Lockfile) -> io::Result<Vec<String>> {
    let mut keys = Vec::new();
    for (key, metadata) in lockfile.packages.iter().flatten() {
        add_resolution_index_keys(&mut keys, key, &metadata.resolution)?;
    }
    Ok(keys)
}

/// The lockfile does not say which key the install used for:
/// - a git-hosted package, which has a key for its built and its unbuilt
///   contents;
/// - a `Variations` resolution, whose variant depends on the installing
///   host and its `supportedArchitectures`;
/// - a runtime, whose row an older install keyed without its manifest.
fn add_resolution_index_keys(
    keys: &mut Vec<String>,
    key: &PackageKey,
    resolution: &LockfileResolution,
) -> io::Result<()> {
    if let LockfileResolution::Variations(variations) = resolution {
        for variant in &variations.variants {
            add_resolution_index_keys(keys, key, &variant.resolution)?;
        }
        return Ok(());
    }
    let pkg_id = key.pkg_id();
    keys.extend(
        [true, false]
            .into_iter()
            .filter_map(|built| store_index_key_for_resolution(resolution, &pkg_id, built)),
    );
    if let LockfileResolution::Binary(binary) = resolution {
        keys.push(binary_store_index_key(key, binary).map_err(io::Error::other)?);
    }
    Ok(())
}

fn row_files(row: &PackageFilesIndex) -> impl Iterator<Item = &CafsFileInfo> {
    let side_effects_files = row.side_effects
        .iter()
        .flat_map(|side_effects| side_effects.values())
        .filter_map(|diff| diff.added.as_ref())
        .flat_map(|added| added.values());
    row.files.values().chain(side_effects_files)
}
