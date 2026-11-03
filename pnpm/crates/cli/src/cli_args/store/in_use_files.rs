//! What the installs of the registered projects use.
//!
//! A package imported by copy or clone, such as a patched or built one, has
//! no hard link to the store, so `store prune` needs the list of its files to
//! keep it. A package linked from a modules directory not named
//! `node_modules` is found only through the projects that the installed
//! lockfiles list.

use pnpm_config::{Host, PackageConfigsSetting, workspace_root_or};
use pnpm_deps_restorer::{binary_store_index_key, store_index_key_for_resolution};
use pnpm_env_replace::env_replace_lossy;
use pnpm_fs::lexical_normalize;
use pnpm_lockfile::{Lockfile, LockfileResolution, PackageKey};
use pnpm_modules_yaml::read_modules_layout;
use pnpm_package_manifest::safe_read_project_manifest_from_dir;
use pnpm_store_dir::{
    CafsFileInfo, InUse, PackageFilesIndex, PruneError, StoreDir, StoreIndex, StoreIndexError,
    decode_package_files_index, get_registered_projects,
};
use pnpm_workspace::read_workspace_manifest_document;
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    io, iter,
    path::{Path, PathBuf},
};

/// For each registered project's current lockfile:
/// - every file of the store index rows of its packages, including the files
///   of every side-effects entry;
/// - the modules directory of every project it lists.
///
/// A project without a current lockfile, a project whose lockfile version
/// this pnpm does not support, and a package without a row add no files. A
/// registry error keeps its own code, and any other failure is
/// [`PruneError::InUseFiles`].
pub(super) fn in_use(store_dir: &StoreDir) -> Result<InUse, PruneError> {
    let mut keys = HashSet::new();
    let mut modules_dirs = ModulesDirs::default();
    for project in get_registered_projects(store_dir).map_err(PruneError::ListProjects)? {
        // The registry joins a relative link target without normalizing it.
        let project = lexical_normalize(&project);
        let modules_dir = modules_dirs.add(&project).map_err(PruneError::InUseFiles)?;
        for lockfile in
            load_installed_lockfiles(&project, &modules_dir).map_err(PruneError::InUseFiles)?
        {
            keys.extend(package_index_keys(&lockfile).map_err(PruneError::InUseFiles)?);
            for importer_id in lockfile.importers.keys() {
                modules_dirs
                    .add(&lexical_normalize(&project.join(importer_id)))
                    .map_err(PruneError::InUseFiles)?;
            }
        }
    }
    Ok(InUse {
        files: in_use_files(store_dir, keys)?,
        modules_dirs: modules_dirs.dirs.into_iter().collect(),
    })
}

fn in_use_files(
    store_dir: &StoreDir,
    keys: HashSet<String>,
) -> Result<HashSet<PathBuf>, PruneError> {
    if keys.is_empty() || !StoreIndex::exists(store_dir.root()) {
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

/// The lockfiles the last install wrote to `modules_dir`, which record what
/// is installed in the project.
///
/// The install writes the lockfile to `<modulesDir>/.pnpm`, or to the
/// `virtualStoreDir` that `<modulesDir>/.modules.yaml` records when the
/// project sets one. Under a global virtual store, that setting names the
/// shared store and the lockfile stays in `<modulesDir>/.pnpm`. The
/// manifest does not say which case applies, so the prune reads both.
fn load_installed_lockfiles(project: &Path, modules_dir: &Path) -> io::Result<Vec<Lockfile>> {
    let default_dir = modules_dir.join(".pnpm");
    let virtual_store_dir = read_modules_layout::<pnpm_modules_yaml::Host>(modules_dir)
        .map_err(io::Error::other)?
        .map(|layout| PathBuf::from(layout.virtual_store_dir))
        .filter(|dir| *dir != default_dir);
    let mut lockfiles = Vec::new();
    for install_state_dir in iter::once(default_dir).chain(virtual_store_dir) {
        lockfiles.extend(load_installed_lockfile(project, &install_state_dir)?);
    }
    Ok(lockfiles)
}

/// The modules directories of the projects added so far, reading each
/// workspace's settings once.
#[derive(Default)]
struct ModulesDirs {
    dirs: HashSet<PathBuf>,
    settings_by_workspace: HashMap<PathBuf, ModulesDirSettings>,
}

impl ModulesDirs {
    /// Adds the modules directory of `project` and returns it.
    fn add(&mut self, project: &Path) -> io::Result<PathBuf> {
        let workspace_dir = workspace_root_or(project);
        if !self.settings_by_workspace.contains_key(&workspace_dir) {
            let settings = ModulesDirSettings::read(&workspace_dir)?;
            self.settings_by_workspace.insert(workspace_dir.clone(), settings);
        }
        let settings = &self.settings_by_workspace[&workspace_dir];
        let modules_dir = project.join(settings.modules_dir_of(project)?);
        self.dirs.insert(modules_dir.clone());
        Ok(modules_dir)
    }
}

/// The settings of a `pnpm-workspace.yaml` that decide the `modulesDir` an
/// install of a project uses. A `modulesDir` set only on the command line,
/// in the environment, or in the global config is not seen.
#[derive(Default)]
struct ModulesDirSettings {
    modules_dir: Option<String>,
    /// The `modulesDir` of each `packageConfigs` entry, by project name.
    /// Empty when the workspace shares one lockfile, as install ignores
    /// them then.
    project_modules_dirs: HashMap<String, String>,
}

impl ModulesDirSettings {
    /// Reads the `pnpm-workspace.yaml` in `workspace_dir`, replacing the
    /// `${VAR}` placeholders of `modulesDir` as install replaces them.
    fn read(workspace_dir: &Path) -> io::Result<Self> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Settings {
            lockfile_dir: Option<String>,
            modules_dir: Option<String>,
            package_configs: Option<PackageConfigsSetting>,
            shared_workspace_lockfile: Option<bool>,
        }
        let Some(document) =
            read_workspace_manifest_document(workspace_dir).map_err(io::Error::other)?
        else {
            return Ok(Self::default());
        };
        let Some(settings) = serde_saphyr::from_str::<Option<Settings>>(&document.text)
            .map_err(|error| io::Error::other(format!("{}: {error}", document.path.display())))?
        else {
            return Ok(Self::default());
        };
        let shares_one_lockfile = settings.shared_workspace_lockfile != Some(false)
            || settings.lockfile_dir.is_some();
        let project_modules_dirs = match settings.package_configs {
            Some(package_configs) if !shares_one_lockfile => package_configs
                .into_record()
                .into_iter()
                .filter_map(|(name, config)| Some((name, config.modules_dir?)))
                .collect(),
            _ => HashMap::new(),
        };
        Ok(Self {
            modules_dir: settings.modules_dir.map(|modules_dir| {
                env_replace_lossy::<Host>(&modules_dir).0
            }),
            project_modules_dirs,
        })
    }

    /// The `modulesDir` of `project`: its `packageConfigs` entry's, the
    /// workspace's, or `node_modules`.
    fn modules_dir_of(&self, project: &Path) -> io::Result<&str> {
        let project_modules_dir = if self.project_modules_dirs.is_empty() {
            None
        } else {
            let manifest = safe_read_project_manifest_from_dir(project).map_err(io::Error::other)?;
            manifest
                .as_ref()
                .and_then(|manifest| manifest.get("name")?.as_str())
                .and_then(|name| self.project_modules_dirs.get(name))
        };
        Ok(project_modules_dir.or(self.modules_dir.as_ref()).map_or("node_modules", String::as_str))
    }
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
/// - a `Variations` resolution, whose variant or fallback depends on the
///   installing host and its `supportedArchitectures`;
/// - a runtime, whose row an older install keyed without its manifest.
fn add_resolution_index_keys(
    keys: &mut Vec<String>,
    key: &PackageKey,
    resolution: &LockfileResolution,
) -> io::Result<()> {
    if let LockfileResolution::Variations(variations) = resolution {
        for resolution in variations.resolutions() {
            add_resolution_index_keys(keys, key, resolution)?;
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
