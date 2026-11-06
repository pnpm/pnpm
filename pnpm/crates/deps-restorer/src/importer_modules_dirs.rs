use pnpm_config::Config;
use pnpm_fs::lexical_normalize;
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

/// The modules directory of each importer of a lockfile.
///
/// The root importer's is the configured `modulesDir`. Every other
/// project's is the directory of the same project-relative name inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImporterModulesDirs {
    lockfile_dir: PathBuf,
    root: PathBuf,
    project_relative: OsString,
    importer_dirs: Vec<PathBuf>,
}

impl ImporterModulesDirs {
    /// `root` is resolved against `lockfile_dir` when relative, and
    /// normalized.
    #[must_use]
    pub fn new(lockfile_dir: &Path, root: &Path, project_relative: &OsStr) -> Self {
        ImporterModulesDirs {
            lockfile_dir: lockfile_dir.to_path_buf(),
            root: lexical_normalize(&lockfile_dir.join(root)),
            project_relative: project_relative.to_os_string(),
            importer_dirs: vec![lexical_normalize(lockfile_dir)],
        }
    }

    /// Every importer keeps its dependencies in `node_modules`.
    #[must_use]
    pub fn node_modules(lockfile_dir: &Path) -> Self {
        Self::new(lockfile_dir, Path::new("node_modules"), OsStr::new("node_modules"))
    }

    #[must_use]
    pub fn from_config(config: &Config, lockfile_dir: &Path) -> Self {
        Self::new(lockfile_dir, &config.modules_dir, config.modules_dir_name())
    }

    /// Records the directories of the lockfile's importers, so
    /// [`Self::holds_importer`] knows every project, including one the
    /// hoisting plan has nothing to place in.
    #[must_use]
    pub fn with_importers<'a>(mut self, importer_ids: impl IntoIterator<Item = &'a str>) -> Self {
        let lockfile_dir = &self.lockfile_dir;
        self.importer_dirs.extend(
            importer_ids
                .into_iter()
                .map(|importer_id| lexical_normalize(&lockfile_dir.join(importer_id))),
        );
        self
    }

    /// Whether `dir` is the lockfile directory, a recorded importer's
    /// directory, or an ancestor of either.
    #[must_use]
    pub fn holds_importer(&self, dir: &Path) -> bool {
        let dir = lexical_normalize(dir);
        self.importer_dirs
            .iter()
            .any(|importer_dir| importer_dir.starts_with(&dir))
    }

    /// The modules directory of the importer whose directory is
    /// `importer_dir`. A project's is joined onto `importer_dir` as given,
    /// so it matches the paths the caller builds from that directory.
    #[must_use]
    pub fn of(&self, importer_dir: &Path) -> PathBuf {
        if lexical_normalize(importer_dir) == lexical_normalize(&self.lockfile_dir) {
            self.root.clone()
        } else {
            importer_dir.join(&self.project_relative)
        }
    }

    /// The modules directory of the importer with the lockfile key
    /// `importer_id`.
    #[must_use]
    pub fn of_importer_id(&self, importer_id: &str) -> PathBuf {
        self.of(&self.lockfile_dir.join(importer_id))
    }

    #[must_use]
    pub fn lockfile_dir(&self) -> &Path {
        &self.lockfile_dir
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests;
