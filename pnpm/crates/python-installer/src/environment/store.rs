//! Where a project's environment generations live, and the link that
//! makes one the project's own.

use miette::{IntoDiagnostic, Result, WrapErr, bail};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// The environments pnpm manages, kept in the store rather than beside
/// their projects: one directory per project, holding one directory per
/// generation, with the project's `.venv` linking to the generation it
/// currently runs.
///
/// A repository with many Python projects therefore holds one link per
/// project rather than a generation directory each, and an environment is
/// on the store's filesystem, where the wheel files it shares with the
/// store can be cloned or hardlinked.
#[derive(Clone)]
pub(crate) struct EnvironmentStore {
    root: PathBuf,
    frozen: bool,
}

impl EnvironmentStore {
    pub(crate) fn new(config: &pnpm_config::Config) -> Self {
        Self { root: config.store_dir.root().join("python-envs"), frozen: config.frozen_store }
    }

    /// A fresh generation for the project at `root`, which publication
    /// makes the project's own once every participant has prepared.
    pub(crate) fn new_generation(&self, root: &Path) -> Result<Generation> {
        self.validate_link(root)?;
        let directory = self.project_directory(root)?;
        fs::create_dir_all(&directory)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!("create Python environment directory {}", directory.display())
            })?;
        // `.venv` links to the generation by this path, so it is made
        // absolute and physical here rather than at every reader.
        let directory = dunce::canonicalize(&directory)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!("resolve Python environment directory {}", directory.display())
            })?;
        let directory = tempfile::Builder::new()
            .prefix("env-")
            .tempdir_in(directory)
            .into_diagnostic()?;
        Ok(Generation { directory, store: self.clone() })
    }

    /// The directory holding the generations of the project at `root`,
    /// named by the project's location so that two projects never share
    /// one. A frozen store is not written, so a project installed from one
    /// keeps its generations beside it, where 12.4 kept them.
    fn project_directory(&self, root: &Path) -> Result<PathBuf> {
        if self.frozen {
            return ensure_beside_project(root);
        }
        let root = dunce::canonicalize(root)
            .into_diagnostic()
            .wrap_err_with(|| format!("resolve Python project directory {}", root.display()))?;
        Ok(self.root.join(pnpm_crypto_hash::create_short_hash(&root.to_string_lossy())))
    }

    /// The generation the project's `.venv` currently links to, or `None`
    /// when it has no environment, or links to one that no longer exists.
    ///
    /// pnpm replaces only a link it made, so a `.venv` that is a directory,
    /// or a link to anything but a generation of pnpm's, is refused. A link
    /// into this store from another project's directory is still pnpm's: a
    /// project that has moved keeps its environment.
    pub(crate) fn validate_link(&self, root: &Path) -> Result<Option<PathBuf>> {
        let link = root.join(".venv");
        match fs::symlink_metadata(&link) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).into_diagnostic(),
            Ok(_) => {
                if !pnpm_fs::is_symlink_or_junction(&link).into_diagnostic()? {
                    bail!(
                        "pnpm will not replace an unmanaged Python environment: {}",
                        link.display(),
                    );
                }
                let Some(target) = link_target(root, &link)? else {
                    return Ok(None);
                };
                if !self.holds(&target)? && !holds_beside_project(root, &target)? {
                    bail!(
                        "pnpm will not replace an unmanaged Python environment: {}",
                        link.display(),
                    );
                }
                Ok(Some(target))
            }
        }
    }

    /// Whether `generation` is a generation directory of a project
    /// directory of this store.
    fn holds(&self, generation: &Path) -> Result<bool> {
        let root = match dunce::canonicalize(&self.root) {
            Ok(root) => root,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| {
                        format!("resolve Python environment store {}", self.root.display())
                    });
            }
        };
        Ok(generation.parent().and_then(Path::parent) == Some(root.as_path()))
    }
}

/// Where `link` leads, or `None` when it leads nowhere: a link to nothing
/// protects nothing, and the store a link led into may have been removed
/// since the environment was published.
fn link_target(root: &Path, link: &Path) -> Result<Option<PathBuf>> {
    let target = root.join(pnpm_fs::read_symlink_dir(link).into_diagnostic()?);
    match dunce::canonicalize(&target) {
        Ok(target) => Ok(Some(target)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!(
                    "resolve Python environment target {} for {}",
                    target.display(),
                    link.display(),
                )
            }),
    }
}

/// Whether `generation` is in the project's own `.pnpm/python-envs`, where
/// pnpm 12.4 published environments and a frozen store still does. One
/// published there is pnpm's to replace as much as one in the store.
fn holds_beside_project(root: &Path, generation: &Path) -> Result<bool> {
    let Some(beside) = existing_beside_project(root)? else {
        return Ok(false);
    };
    let beside = dunce::canonicalize(&beside).into_diagnostic()?;
    Ok(generation.parent() == Some(beside.as_path()))
}

/// The project's own `.pnpm/python-envs`, created. A checkout can carry a
/// `.pnpm` that is a symlink, and pnpm writes nothing through one.
fn ensure_beside_project(root: &Path) -> Result<PathBuf> {
    let mut path = root.to_path_buf();
    for component in [".pnpm", "python-envs"] {
        path.push(component);
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error).into_diagnostic(),
        }
        if !is_real_directory(&path)? {
            bail!("managed Python directory must be a real directory: {}", path.display());
        }
    }
    Ok(path)
}

/// The project's own `.pnpm/python-envs` when it exists, reached through
/// no symlink: 12.4 published nothing through one, so a generation reached
/// that way is not its.
fn existing_beside_project(root: &Path) -> Result<Option<PathBuf>> {
    let mut path = root.to_path_buf();
    for component in [".pnpm", "python-envs"] {
        path.push(component);
        if !is_real_directory(&path)? {
            return Ok(None);
        }
    }
    Ok(Some(path))
}

fn is_real_directory(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            Ok(metadata.is_dir() && !pnpm_fs::is_symlink_or_junction(path).into_diagnostic()?)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).into_diagnostic(),
    }
}

/// A generation prepared in the store, and the store it was prepared in,
/// which publication asks whether the project's `.venv` is a link pnpm may
/// replace.
pub(crate) struct Generation {
    pub(crate) directory: tempfile::TempDir,
    pub(crate) store: EnvironmentStore,
}

pub(crate) fn publish_link(root: &Path, target: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        let outcome =
            pnpm_fs::force_absolute_symlink_dir(target, &root.join(".venv")).into_diagnostic()?;
        if let Some(warning) = outcome.warning {
            bail!("{warning}");
        }
        Ok(())
    }
    #[cfg(unix)]
    {
        let temporary = tempfile::Builder::new()
            .prefix(".pnpm-python-link-")
            .tempdir_in(root)
            .into_diagnostic()?;
        let staged = temporary.path().join(".venv");
        // The link is moved up one level when published, so relative links must
        // be computed from their final location, not from the temporary directory.
        std::os::unix::fs::symlink(target, &staged).into_diagnostic()?;
        fs::rename(&staged, root.join(".venv")).into_diagnostic()
    }
}
