//! Python environments in the store, at
//! `<store_dir>/python-envs/<short-hash>/env-*`.
//!
//! - `<short-hash>` is one project's directory. The hash of the project's
//!   canonical path names it.
//! - Each `env-*` directory under it is one generation, made by one install.
//!   The project's `.venv` links to the generation it runs.
//! - The [`PROJECT_LINK`] link in the project's directory leads back to the
//!   project. `store prune` reads it to learn which generation is in use.

use crate::{
    RegisterProjectError,
    StoreDir,
    project_registry::link_to_project,
};
use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use pnpm_crypto_hash::create_short_hash;
use std::{
    fs,
    io,
    path::{
        Path,
        PathBuf,
    },
};

/// The link in a project's environment directory that leads back to the
/// project.
pub(crate) const PROJECT_LINK: &str = "project";

/// The name prefix of every generation directory.
pub(crate) const GENERATION_PREFIX: &str = "env-";

/// Error type of [`StoreDir::python_environment_dir`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum PythonEnvironmentDirError {
    #[display("Failed to create the Python environment directory at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_STORE_DIR_CREATE_PYTHON_ENVIRONMENT_DIR))]
    Create {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[diagnostic(transparent)]
    LinkProject(#[error(source)] RegisterProjectError),
}

impl StoreDir {
    /// The directory of every project's Python environments.
    pub fn python_envs(&self) -> PathBuf {
        self.root().join("python-envs")
    }

    /// The directory of the environment generations of the project at the
    /// canonical path `project_root`, created, with a link back to the
    /// project.
    pub fn python_environment_dir(
        &self,
        project_root: &Path,
    ) -> Result<PathBuf, PythonEnvironmentDirError> {
        let dir = self
            .python_envs()
            .join(create_short_hash(&project_root.to_string_lossy()));
        fs::create_dir_all(&dir)
            .map_err(|error| PythonEnvironmentDirError::Create { path: dir.clone(), error })?;
        link_to_project(project_root, dir.join(PROJECT_LINK))
            .map_err(PythonEnvironmentDirError::LinkProject)?;
        Ok(dir)
    }
}
