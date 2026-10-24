//! The store files of the packages that registered projects installed.
//!
//! A package imported by copy or clone, such as a patched or built one, has
//! no hard link to the store, so `store prune` needs this list to keep it.

use pnpm_deps_restorer::{binary_store_index_key, store_index_key_for_resolution};
use pnpm_lockfile::{Lockfile, LockfileResolution, PackageKey};
use pnpm_modules_yaml::{Host, read_modules_layout};
use pnpm_store_dir::{
    CafsFileInfo, PackageFilesIndex, PruneError, StoreDir, StoreIndex, get_registered_projects,
};
use std::{
    collections::HashSet,
    io, iter,
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
        for lockfile in load_installed_lockfiles(&project).map_err(PruneError::InUseFiles)? {
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

/// The lockfiles the last install wrote, which record what is installed in
/// the project.
///
/// The install writes the lockfile to `node_modules/.pnpm`, or to the
/// `virtualStoreDir` that `node_modules/.modules.yaml` records when the
/// project sets one. Under a global virtual store, that setting names the
/// shared store and the lockfile stays in `node_modules/.pnpm`. The
/// manifest does not say which case applies, so the prune reads both.
fn load_installed_lockfiles(project: &Path) -> io::Result<Vec<Lockfile>> {
    // A project with a custom modules directory has no manifest or lockfile
    // here, so only their hard links keep its packages.
    let modules_dir = project.join("node_modules");
    let default_dir = modules_dir.join(".pnpm");
    let virtual_store_dir = read_modules_layout::<Host>(&modules_dir)
        .map_err(io::Error::other)?
        .map(|layout| PathBuf::from(layout.virtual_store_dir))
        .filter(|dir| *dir != default_dir);
    let mut lockfiles = Vec::new();
    for install_state_dir in iter::once(default_dir).chain(virtual_store_dir) {
        let lockfile = Lockfile::load_current_from_install_state_dir(&install_state_dir)
            .map_err(|error| {
                io::Error::other(format!("{}: {error}", install_state_dir.display()))
            })?;
        lockfiles.extend(lockfile);
    }
    Ok(lockfiles)
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
