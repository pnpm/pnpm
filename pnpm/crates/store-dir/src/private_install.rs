//! A private install: one process's own copy of a pinned package manager
//! or runtime, made when the shared global-virtual-store slot the copy
//! would otherwise be installed into is held by another process.
//!
//! The copy lives under `<store>/tmp/private` and is removed when its
//! [`PrivateInstall`] handle is dropped. A process that ends without
//! dropping the handle — killed, or replaced by the program it
//! executes — leaves the directory behind, and `pnpm store prune`
//! removes it then. Prune tells a copy still in use from one left behind
//! by the OS lock the handle holds on the `in-use` file inside the
//! directory: the OS releases that lock when the holding process ends,
//! however it ends.
//!
//! A directory is marked in use only after it is created, so creation
//! runs under the store's use lock and the sweep under its prune lock,
//! the way every other store consumer and prune keep out of each
//! other's way.

use crate::{StoreDir, StoreLockError};
use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    fs::{self, File},
    io,
    path::{Component, Path, PathBuf},
};

/// The directory under the store's `tmp/` that holds the private installs.
pub(crate) const PRIVATE_INSTALLS_DIR: &str = "private";

/// The file inside a private install whose OS lock marks it in use.
const IN_USE_FILE: &str = "in-use";

#[derive(Debug, Display, Error, Diagnostic)]
pub enum PrivateInstallError {
    #[diagnostic(transparent)]
    StoreLock(#[error(source)] StoreLockError),

    #[display("A private install label must be a single path component, not {label:?}")]
    #[diagnostic(code(ERR_PNPM_PRIVATE_INSTALL_LABEL))]
    InvalidLabel { label: String },

    #[display("Failed to create the private install at {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_PRIVATE_INSTALL_CREATE))]
    Create {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to remove the private installs left behind under {path:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_PRIVATE_INSTALL_REMOVE))]
    Remove {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

/// A private install directory, in use by this process until dropped,
/// which removes it. A caller that ends the process itself, through
/// `exit`, drops the handle first: `exit` runs no destructors.
#[derive(Debug)]
pub struct PrivateInstall {
    dir: PathBuf,
    /// The OS lock marking the directory in use. `None` on a filesystem
    /// that cannot hold one; prune then leaves the directory to whoever
    /// holds the handle.
    in_use: Option<File>,
}

impl PrivateInstall {
    /// The empty directory to install into.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Drop for PrivateInstall {
    fn drop(&mut self) {
        // Released first: Windows refuses to remove a locked file.
        drop(self.in_use.take());
        let _ = fs::remove_dir_all(&self.dir);
    }
}

impl StoreDir {
    /// Create an empty directory for a private install of `label` (a
    /// single path component naming what is installed, for anyone
    /// reading the store), marked in use by this process until the
    /// returned handle is dropped.
    pub fn create_private_install(
        &self,
        label: &str,
    ) -> Result<PrivateInstall, PrivateInstallError> {
        if !is_single_normal_component(label) {
            return Err(PrivateInstallError::InvalidLabel { label: label.to_string() });
        }
        let _store_lock = self.lock_for_use().map_err(PrivateInstallError::StoreLock)?;
        let parent = self.private_installs_dir();
        let dir = parent.join(crate::unique_dir_name(label));
        // Exclusive creation: a directory that already exists belongs to
        // another install, and two handles over one directory would each
        // remove the other's copy.
        fs::create_dir_all(&parent)
            .and_then(|()| fs::create_dir(&dir))
            .map_err(|error| PrivateInstallError::Create { path: dir.clone(), error })?;
        let marker = dir.join(IN_USE_FILE);
        let in_use = pnpm_fs::open_secure_lock_file(&marker)
            .map_err(|error| PrivateInstallError::Create { path: marker, error })?;
        let in_use = in_use.lock().is_ok().then_some(in_use);
        Ok(PrivateInstall { dir, in_use })
    }

    /// Remove every private install under this store that no process
    /// holds any more, and report how many went.
    pub fn prune_private_installs(&self) -> Result<usize, PrivateInstallError> {
        let _store_lock = self.lock_for_prune().map_err(PrivateInstallError::StoreLock)?;
        self.remove_orphaned_private_installs()
            .map_err(|error| PrivateInstallError::Remove {
                path: self.private_installs_dir(),
                error,
            })
    }

    /// [`Self::prune_private_installs`] for a caller that already holds
    /// the store's prune lock.
    pub(crate) fn remove_orphaned_private_installs(&self) -> io::Result<usize> {
        let dir = self.private_installs_dir();
        let Some(entries) = read_private_installs_dir(&dir)? else {
            return Ok(0);
        };
        let mut removed = 0;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            // Only a real directory can be a private install a process
            // holds; anything else under here is litter.
            if entry.file_type()?.is_dir() && is_in_use(&path)? {
                continue;
            }
            pnpm_fs::remove_dirent(&path)?;
            removed += 1;
        }
        // Best-effort: the directory stays when a private install is
        // still held.
        let _ = fs::remove_dir(&dir);
        Ok(removed)
    }

    fn private_installs_dir(&self) -> PathBuf {
        self.tmp().join(PRIVATE_INSTALLS_DIR)
    }
}

fn is_single_normal_component(label: &str) -> bool {
    let mut components = Path::new(label).components();
    matches!((components.next(), components.next()), (Some(Component::Normal(_)), None))
}

/// The entries of the private installs directory, `None` when there is
/// none. A symbolic link in its place is refused rather than followed,
/// since the sweep removes what it finds.
fn read_private_installs_dir(dir: &Path) -> io::Result<Option<fs::ReadDir>> {
    match fs::symlink_metadata(dir) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the private installs directory must not be a symbolic link",
        )),
        Ok(_) => fs::read_dir(dir).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Whether a process still holds the private install at `dir`. One with
/// no `in-use` file was abandoned before it was marked. One whose lock
/// cannot be probed is left alone: on a filesystem without OS locks its
/// holder cannot be told from a process that is gone.
fn is_in_use(dir: &Path) -> io::Result<bool> {
    let in_use = match File::open(dir.join(IN_USE_FILE)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    Ok(in_use.try_lock().is_err())
}

#[cfg(test)]
mod tests;
