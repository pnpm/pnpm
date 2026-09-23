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

use crate::StoreDir;
use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};

/// The directory under the store's `tmp/` that holds the private installs.
pub(crate) const PRIVATE_INSTALLS_DIR: &str = "private";

/// The file inside a private install whose OS lock marks it in use.
const IN_USE_FILE: &str = "in-use";

/// A private install directory, in use by this process until dropped.
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
    pub fn create_private_install(&self, label: &str) -> io::Result<PrivateInstall> {
        let dir = self
            .private_installs_dir()
            .join(crate::unique_dir_name(label));
        fs::create_dir_all(&dir)?;
        let in_use = pnpm_fs::open_secure_lock_file(&dir.join(IN_USE_FILE))?;
        let in_use = in_use.lock().is_ok().then_some(in_use);
        Ok(PrivateInstall { dir, in_use })
    }

    /// Remove every private install under this store that no process
    /// holds any more, and report how many went.
    pub fn remove_orphaned_private_installs(&self) -> io::Result<usize> {
        let dir = self.private_installs_dir();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        let mut removed = 0;
        for entry in entries {
            let path = entry?.path();
            if is_in_use(&path)? {
                continue;
            }
            pnpm_fs::remove_dirent(&path)?;
            removed += 1;
        }
        // Best-effort: the directory stays when a private install landed
        // in it meanwhile.
        let _ = fs::remove_dir(&dir);
        Ok(removed)
    }

    fn private_installs_dir(&self) -> PathBuf {
        self.tmp().join(PRIVATE_INSTALLS_DIR)
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
