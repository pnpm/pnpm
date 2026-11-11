#[cfg(not(unix))]
use crate::remove_dirent;
use std::{
    ffi::{OsStr, OsString},
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

#[cfg(unix)]
mod unix;

/// A directory held open while its children are opened or removed.
///
/// On Unix a child is reached through the open directory, so a link put in
/// the directory's place after the open is not followed. On other
/// platforms every operation resolves the directory's path again.
#[derive(Debug)]
pub struct OpenDir {
    path: PathBuf,
    #[cfg(unix)]
    handle: fs::File,
}

impl OpenDir {
    /// Open the directory at `path`, following a symbolic link in its
    /// place. Fails with `NotFound` when nothing is there and with
    /// `NotADirectory` when something else is.
    #[cfg(unix)]
    pub fn open(path: &Path) -> io::Result<Self> {
        let handle = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY)
            .open(path)?;
        Ok(Self { path: path.to_path_buf(), handle })
    }

    #[cfg(not(unix))]
    pub fn open(path: &Path) -> io::Result<Self> {
        if fs::metadata(path)?.is_dir() {
            Ok(Self { path: path.to_path_buf() })
        } else {
            Err(io::Error::from(ErrorKind::NotADirectory))
        }
    }

    /// Open the directory `name`, directly inside this one, or `None` when
    /// `name` is not a directory: a file, a symbolic link, or nothing.
    #[cfg(unix)]
    pub fn open_child_dir(&self, name: &OsStr) -> io::Result<Option<Self>> {
        match pnpm_ffi_fs::open_directory_at(&self.handle, name) {
            Ok(handle) => Ok(Some(Self { path: self.path.join(name), handle })),
            Err(error) if unix::is_not_a_real_dir(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }

    #[cfg(not(unix))]
    pub fn open_child_dir(&self, name: &OsStr) -> io::Result<Option<Self>> {
        let path = self.path.join(name);
        if is_dir_no_follow(&path)? { Ok(Some(Self { path })) } else { Ok(None) }
    }

    /// The names of the entries directly inside this directory, `.` and
    /// `..` excluded.
    pub fn entry_names(&self) -> io::Result<Vec<OsString>> {
        #[cfg(unix)]
        return pnpm_ffi_fs::directory_entry_names(&self.handle);
        #[cfg(not(unix))]
        {
            let entries = match fs::read_dir(&self.path) {
                Ok(entries) => entries,
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
                Err(error) => return Err(error),
            };
            entries
                .map(|entry| entry.map(|entry| entry.file_name()))
                .collect()
        }
    }

    /// The path the directory was opened at.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The absolute path of this directory with every symbolic link
    /// resolved, or `None` when its path no longer leads to it.
    #[cfg(unix)]
    pub fn canonical_path(&self) -> io::Result<Option<PathBuf>> {
        let Some(canonical) = canonicalize(&self.path)? else {
            return Ok(None);
        };
        let opened = self.handle.metadata()?;
        let named = match fs::metadata(&canonical) {
            Ok(named) => named,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let same = opened.dev() == named.dev() && opened.ino() == named.ino();
        Ok(same.then_some(canonical))
    }

    #[cfg(not(unix))]
    pub fn canonical_path(&self) -> io::Result<Option<PathBuf>> {
        Ok(canonicalize(&self.path)?.filter(|canonical| canonical.is_dir()))
    }

    /// Remove the directory `name`, directly inside this one, with
    /// everything in it. Returns `false` and leaves the entry in place when
    /// `name` is not a directory: a file, a symbolic link, or nothing.
    #[cfg(unix)]
    pub fn remove_child_dir(&self, name: &OsStr) -> io::Result<bool> {
        match pnpm_ffi_fs::open_directory_at(&self.handle, name) {
            Ok(dir) => unix::remove_dir_all_at(&self.handle, name, &dir).map(|()| true),
            Err(error) if unix::is_not_a_real_dir(&error) => Ok(false),
            Err(error) => Err(error),
        }
    }

    #[cfg(not(unix))]
    pub fn remove_child_dir(&self, name: &OsStr) -> io::Result<bool> {
        let path = self.path.join(name);
        if !is_dir_no_follow(&path)? {
            return Ok(false);
        }
        match remove_dirent(&path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
            Err(error) => Err(error),
        }
    }
}

/// The canonical form of `path`, or `None` when nothing is there.
fn canonicalize(path: &Path) -> io::Result<Option<PathBuf>> {
    match dunce::canonicalize(path) {
        Ok(canonical) => Ok(Some(canonical)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Whether a directory, and not a link to one, is at `path`.
#[cfg(not(unix))]
fn is_dir_no_follow(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.file_type().is_dir()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests;
