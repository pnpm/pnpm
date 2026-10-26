//! The store files of the packages that registered projects installed.
//!
//! A package imported by copy or clone, such as a patched or built one, has
//! no hard link to the store, so `store prune` needs this list to keep it.

use pnpm_deps_restorer::{binary_store_index_key, store_index_key_for_resolution};
use pnpm_fs::lexical_normalize;
use pnpm_lockfile::{Lockfile, LockfileResolution, PackageKey};
use pnpm_modules_yaml::{Host, read_modules_layout};
use pnpm_store_dir::{
    CafsFileInfo, PackageFilesIndex, PruneError, StoreDir, StoreIndex, StoreIndexError,
    decode_package_files_index, get_registered_projects,
};
use pnpm_workspace::read_workspace_manifest_document;
use serde::Deserialize;
use std::{
    collections::HashSet,
    io, iter,
    path::{Path, PathBuf},
};

/// Every file of the store index rows of the packages in each registered
/// project's current lockfile, including the files of every side-effects
/// entry. A project without a current lockfile, a project whose lockfile
/// version this pnpm does not support, and a package without a row add
/// nothing. A registry error keeps its own code, and any other
/// failure is [`PruneError::InUseFiles`].
pub(super) fn in_use_files(store_dir: &StoreDir) -> Result<HashSet<PathBuf>, PruneError> {
    if !StoreIndex::exists(store_dir.root()) {
        return Ok(HashSet::new());
    }
    let mut keys = HashSet::new();
    for project in get_registered_projects(store_dir).map_err(PruneError::ListProjects)? {
        // The registry joins a relative link target without normalizing it.
        let project = lexical_normalize(&project);
        for lockfile in load_installed_lockfiles(&project).map_err(PruneError::InUseFiles)? {
            keys.extend(package_index_keys(&lockfile).map_err(PruneError::InUseFiles)?);
        }
    }
    if keys.is_empty() {
        return Ok(HashSet::new());
    }
    let keys: Vec<String> = keys.into_iter().collect();
    let rows = read_rows(store_dir, &keys)
        .map_err(|error| PruneError::InUseFiles(io::Error::other(error)))?;
    Ok(rows
        .iter()
        .flat_map(row_files)
        .filter_map(|file| store_dir.cas_file_path_by_mode(&file.digest, file.mode))
        .collect())
}

/// Fails on an undecodable row, which [`StoreIndex::get_many`] skips,
/// because skipping it would let the prune remove the row's files.
fn read_rows(
    store_dir: &StoreDir,
    keys: &[String],
) -> Result<Vec<PackageFilesIndex>, StoreIndexError> {
    StoreIndex::open_readonly_in(store_dir)?
        .get_many_raw(keys)?
        .iter()
        .map(|(_, bytes)| decode_package_files_index(bytes))
        .collect()
}

/// The lockfiles the last install wrote, which record what is installed in
/// the project.
///
/// The install writes the lockfile to `<modulesDir>/.pnpm`, or to the
/// `virtualStoreDir` that `<modulesDir>/.modules.yaml` records when the
/// project sets one. Under a global virtual store, that setting names the
/// shared store and the lockfile stays in `<modulesDir>/.pnpm`. The
/// manifest does not say which case applies, so the prune reads both.
fn load_installed_lockfiles(project: &Path) -> io::Result<Vec<Lockfile>> {
    let modules_dir = project.join(configured_modules_dir(project)?);
    let default_dir = modules_dir.join(".pnpm");
    let virtual_store_dir = read_modules_layout::<Host>(&modules_dir)
        .map_err(io::Error::other)?
        .map(|layout| PathBuf::from(layout.virtual_store_dir))
        .filter(|dir| *dir != default_dir);
    let mut lockfiles = Vec::new();
    for install_state_dir in iter::once(default_dir).chain(virtual_store_dir) {
        lockfiles.extend(load_installed_lockfile(project, &install_state_dir)?);
    }
    Ok(lockfiles)
}

/// The `modulesDir` that the project's `pnpm-workspace.yaml` sets, or
/// `node_modules`. A `modulesDir` set only on the command line, in the
/// environment, or in the global config is not seen.
fn configured_modules_dir(project: &Path) -> io::Result<String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ModulesDirSetting {
        modules_dir: Option<String>,
    }
    let Some(document) = read_workspace_manifest_document(project).map_err(io::Error::other)?
    else {
        return Ok("node_modules".to_string());
    };
    let setting: Option<ModulesDirSetting> = serde_saphyr::from_str(&document.text)
        .map_err(|error| io::Error::other(format!("{}: {error}", document.path.display())))?;
    Ok(setting
        .and_then(|setting| setting.modules_dir)
        .unwrap_or_else(|| "node_modules".to_string()))
}

/// A lockfile of a version this pnpm cannot read adds nothing, like a
/// missing one, so one project installed by another pnpm does not stop the
/// prune. The hard-link check then decides for that project's files.
fn load_installed_lockfile(
    project: &Path,
    install_state_dir: &Path,
) -> io::Result<Option<Lockfile>> {
    let error = match Lockfile::load_current_from_install_state_dir(install_state_dir) {
        Ok(lockfile) => return Ok(lockfile),
        Err(error) => error,
    };
    let lockfile_path = install_state_dir.join(Lockfile::CURRENT_FILE_NAME);
    if let Ok(Some(version)) = Lockfile::unsupported_version_at(&lockfile_path) {
        pnpm_diagnostics::eprintln!(
            "Skipped {} because it was installed with lockfileVersion {version}, \
             which this version of pnpm does not support",
            project.display(),
        );
        return Ok(None);
    }
    Err(io::Error::other(format!("{}: {error}", install_state_dir.display())))
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
