//! The store files of the packages that registered projects installed.
//!
//! A package imported by copy or clone, such as a patched or built one, has
//! no hard link to the store, so `store prune` needs this list to keep it.

use pnpm_deps_restorer::store_index_key_for_resolution;
use pnpm_lockfile::Lockfile;
use pnpm_store_dir::{
    CafsFileInfo, PackageFilesIndex, StoreDir, StoreIndex, get_registered_projects,
};
use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
};

/// Every file of the store index rows of the packages in each registered
/// project's current lockfile, including the files of every side-effects
/// entry. A project without a current lockfile and a package without a row
/// add nothing.
pub(super) fn in_use_files(store_dir: &StoreDir) -> io::Result<HashSet<PathBuf>> {
    if !StoreIndex::exists(store_dir.root()) {
        return Ok(HashSet::new());
    }
    let mut keys = HashSet::new();
    for project in get_registered_projects(store_dir).map_err(io::Error::other)? {
        if let Some(lockfile) = load_installed_lockfile(&project)? {
            keys.extend(package_index_keys(&lockfile));
        }
    }
    if keys.is_empty() {
        return Ok(HashSet::new());
    }
    let keys: Vec<String> = keys.into_iter().collect();
    let rows = StoreIndex::open_readonly_in(store_dir)
        .and_then(|index| index.get_many(&keys))
        .map_err(io::Error::other)?;
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
    // its packages are kept only by their hard links.
    let install_state_dir = project.join("node_modules").join(".pnpm");
    Lockfile::load_current_from_install_state_dir(&install_state_dir)
        .map_err(|error| io::Error::other(format!("{}: {error}", install_state_dir.display())))
}

/// A git-hosted package has a key for its built and its unbuilt contents,
/// and the lockfile does not say which one the install used.
fn package_index_keys(lockfile: &Lockfile) -> impl Iterator<Item = String> + '_ {
    lockfile.packages
        .iter()
        .flatten()
        .flat_map(|(key, metadata)| {
            let pkg_id = key.pkg_id();
            [true, false]
                .into_iter()
                .filter_map(move |built| {
                    store_index_key_for_resolution(&metadata.resolution, &pkg_id, built)
                })
        })
}

fn row_files(row: &PackageFilesIndex) -> impl Iterator<Item = &CafsFileInfo> {
    let side_effects_files = row.side_effects
        .iter()
        .flat_map(|side_effects| side_effects.values())
        .filter_map(|diff| diff.added.as_ref())
        .flat_map(|added| added.values());
    row.files.values().chain(side_effects_files)
}
