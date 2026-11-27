//! Where a project's environment generations live, and the link that
//! makes one the project's own.

use miette::{
    IntoDiagnostic,
    Result,
    WrapErr,
    bail,
};
use std::{
    fs,
    io,
    path::{
        Path,
        PathBuf,
    },
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
    store_dir: pnpm_store_dir::StoreDir,
    frozen: bool,
}

/// What a project's `.venv` is to pnpm.
pub(crate) enum EnvironmentLink {
    /// There is no `.venv`, or it links to an environment that no longer
    /// exists.
    Absent,
    /// A link to this generation of pnpm's. A link into the store from
    /// another project's directory is still pnpm's: a project that has
    /// moved keeps its environment.
    Managed(PathBuf),
    /// A directory, or a link to anything but a generation of pnpm's.
    Unmanaged,
}

impl EnvironmentStore {
    pub(crate) fn new(config: &pnpm_config::Config) -> Self {
        Self { store_dir: config.store_dir.clone(), frozen: config.frozen_store }
    }

    /// A fresh generation for the project at `root`, which publication
    /// makes the project's own once every participant has prepared.
    pub(crate) fn new_generation(&self, root: &Path) -> Result<Generation> {
        self.validate_link(root)?;
        let directory = self.project_directory(root)?;
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
    /// created. A frozen store is not written, so a project installed from
    /// one keeps its generations beside it, where 12.4 kept them.
    fn project_directory(&self, root: &Path) -> Result<PathBuf> {
        if self.frozen {
            return ensure_beside_project(root.to_path_buf());
        }
        let root = dunce::canonicalize(root)
            .into_diagnostic()
            .wrap_err_with(|| format!("resolve Python project directory {}", root.display()))?;
        Ok(self.store_dir.python_environment_dir(&root)?)
    }

    /// The generation the project's `.venv` currently links to, or `None`
    /// when it has no environment. pnpm replaces only a link it made, so
    /// an [`EnvironmentLink::Unmanaged`] one is refused.
    pub(crate) fn validate_link(&self, root: &Path) -> Result<Option<PathBuf>> {
        match self.link(root)? {
            EnvironmentLink::Absent => Ok(None),
            EnvironmentLink::Managed(generation) => Ok(Some(generation)),
            EnvironmentLink::Unmanaged => {
                let link = root.join(".venv");
                bail!("pnpm will not replace an unmanaged Python environment: {}", link.display());
            }
        }
    }

    pub(crate) fn link(&self, root: &Path) -> Result<EnvironmentLink> {
        let link = root.join(".venv");
        match fs::symlink_metadata(&link) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(EnvironmentLink::Absent),
            Err(error) => Err(error).into_diagnostic(),
            Ok(_) => {
                if !pnpm_fs::is_symlink_or_junction(&link).into_diagnostic()? {
                    return Ok(EnvironmentLink::Unmanaged);
                }
                let Some(target) = link_target(root, &link)? else {
                    return Ok(EnvironmentLink::Absent);
                };
                if !self.holds(&target)? && !holds_beside_project(root.to_path_buf(), &target)? {
                    return Ok(EnvironmentLink::Unmanaged);
                }
                Ok(EnvironmentLink::Managed(target))
            }
        }
    }

    /// Whether `generation` is a generation directory of a project
    /// directory of this store.
    fn holds(&self, generation: &Path) -> Result<bool> {
        let environments = self.store_dir.python_envs();
        let root = match dunce::canonicalize(&environments) {
            Ok(root) => root,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| {
                        format!("resolve Python environment store {}", environments.display())
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
fn holds_beside_project(root: PathBuf, generation: &Path) -> Result<bool> {
    let Some(beside) = existing_beside_project(root)? else {
        return Ok(false);
    };
    let beside = dunce::canonicalize(&beside).into_diagnostic()?;
    Ok(generation.parent() == Some(beside.as_path()))
}

/// The project's own `.pnpm/python-envs`, created. A checkout can carry a
/// `.pnpm` that is a symlink, and pnpm writes nothing through one.
fn ensure_beside_project(root: PathBuf) -> Result<PathBuf> {
    let mut path = root;
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
fn existing_beside_project(root: PathBuf) -> Result<Option<PathBuf>> {
    let mut path = root;
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
    #[cfg(any(unix, target_os = "wasi"))]
    {
        let temporary = tempfile::Builder::new()
            .prefix(".pnpm-python-link-")
            .tempdir_in(root)
            .into_diagnostic()?;
        let staged = temporary.path().join(".venv");
        // The link is moved up one level when published, so relative links must
        // be computed from their final location, not from the temporary directory.
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, &staged).into_diagnostic()?;
        #[cfg(target_os = "wasi")]
        pnpm_fs::create_symlink(target, &staged, true).into_diagnostic()?;
        fs::rename(&staged, root.join(".venv")).into_diagnostic()
    }
}
