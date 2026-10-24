#[cfg(not(unix))]
use crate::remove_dirent;
use std::{
    ffi::OsStr,
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::{
    ffi::CString,
    os::{
        fd::AsFd,
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
};

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
        match unix::open_dir_at(self.handle.as_fd(), &c_name(name)?) {
            Ok(handle) => Ok(Some(Self { path: self.path.join(name), handle: handle.into() })),
            Err(error) if unix::is_not_a_real_dir(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }

    #[cfg(not(unix))]
    pub fn open_child_dir(&self, name: &OsStr) -> io::Result<Option<Self>> {
        let path = self.path.join(name);
        if is_dir_no_follow(&path)? { Ok(Some(Self { path })) } else { Ok(None) }
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
        let name = c_name(name)?;
        let parent = self.handle.as_fd();
        match unix::open_dir_at(parent, &name) {
            Ok(dir) => unix::remove_dir_all_at(parent, &name, dir).map(|()| true),
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

#[cfg(unix)]
fn c_name(name: &OsStr) -> io::Result<CString> {
    Ok(CString::new(name.as_bytes())?)
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
