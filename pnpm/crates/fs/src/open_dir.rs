#[cfg(not(any(unix, windows)))]
use crate::remove_dirent;
use std::{
    ffi::{OsStr, OsString},
    fs,
    io::{self, ErrorKind},
    path::{Component, Path, PathBuf},
    time::SystemTime,
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
#[cfg(windows)]
mod windows;

/// Size and timestamps of a regular file inspected through [`OpenDir`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildFileStat {
    pub len: u64,
    pub modified: SystemTime,
    pub accessed: SystemTime,
}

/// A directory held open while its children are opened or removed.
///
/// On Unix a child is reached through the open directory descriptor, so a link
/// put in the directory's place after the open is not followed. On Windows
/// every opened component is held without `FILE_SHARE_DELETE`, preventing
/// ancestors from being renamed or replaced while child operations run.
#[derive(Debug)]
pub struct OpenDir {
    path: PathBuf,
    #[cfg(unix)]
    handle: fs::File,
    #[cfg(windows)]
    pinned: windows::PinnedChain,
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

    #[cfg(windows)]
    pub fn open(path: &Path) -> io::Result<Self> {
        let pinned = windows::open_root(path)?;
        Ok(Self { path: path.to_path_buf(), pinned })
    }

    #[cfg(not(any(unix, windows)))]
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

    #[cfg(windows)]
    pub fn open_child_dir(&self, name: &OsStr) -> io::Result<Option<Self>> {
        Ok(windows::open_child_dir(&self.path, &self.pinned, name)?
            .map(|(path, pinned)| Self { path, pinned }))
    }

    #[cfg(not(any(unix, windows)))]
    pub fn open_child_dir(&self, name: &OsStr) -> io::Result<Option<Self>> {
        let path = self.path.join(name);
        if is_dir_no_follow(&path)? { Ok(Some(Self { path })) } else { Ok(None) }
    }

    /// The names of the entries directly inside this directory, `.` and
    /// `..` excluded.
    pub fn entry_names(&self) -> io::Result<Vec<OsString>> {
        #[cfg(unix)]
        return unix::entry_names(self.handle.as_fd());
        #[cfg(windows)]
        return read_dir_entry_names(self.pinned.path());
        #[cfg(not(any(unix, windows)))]
        return read_dir_entry_names(&self.path);
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

    #[cfg(windows)]
    pub fn canonical_path(&self) -> io::Result<Option<PathBuf>> {
        let Some(canonical) = canonicalize(&self.path)? else {
            return Ok(None);
        };
        Ok((canonical == self.pinned.path() && canonical.is_dir()).then_some(canonical))
    }

    #[cfg(not(any(unix, windows)))]
    pub fn canonical_path(&self) -> io::Result<Option<PathBuf>> {
        Ok(canonicalize(&self.path)?.filter(|canonical| canonical.is_dir()))
    }

    /// Duplicate this directory handle so a caller can own an independent
    /// [`OpenDir`] anchored to the same directory.
    pub fn try_reopen(&self) -> io::Result<Self> {
        #[cfg(unix)]
        {
            let handle = unix::open_dir_at(self.handle.as_fd(), c".")?;
            Ok(Self { path: self.path.clone(), handle: handle.into() })
        }
        #[cfg(windows)]
        {
            Ok(Self { path: self.path.clone(), pinned: self.pinned.clone() })
        }
        #[cfg(not(any(unix, windows)))]
        {
            Self::open(&self.path)
        }
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

    #[cfg(windows)]
    pub fn remove_child_dir(&self, name: &OsStr) -> io::Result<bool> {
        windows::remove_child_dir(self, name)
    }

    #[cfg(not(any(unix, windows)))]
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

    /// Open a relative directory path inside `self`, stepping through each
    /// normal component without following symbolic links. Returns `None` if any
    /// segment is missing, a file, a symbolic link, or not a normal component.
    pub fn open_descendant_dir(&self, rel: &Path) -> io::Result<Option<Self>> {
        let mut current: Option<Self> = None;
        for component in rel.components() {
            match component {
                Component::CurDir => {}
                Component::Normal(part) => {
                    let base = current.as_ref().unwrap_or(self);
                    let Some(next) = base.open_child_dir(part)? else {
                        return Ok(None);
                    };
                    current = Some(next);
                }
                _ => return Ok(None),
            }
        }
        if let Some(dir) = current { Ok(Some(dir)) } else { self.try_reopen().map(Some) }
    }

    /// Inspect the regular file `name` directly inside this directory without
    /// following symbolic links. Returns `None` for directories, symbolic
    /// links, special files, and missing entries.
    #[cfg(unix)]
    pub fn child_file_stat(&self, name: &OsStr) -> io::Result<Option<ChildFileStat>> {
        unix::stat_file_at(self.handle.as_fd(), &c_name(name)?)
    }

    #[cfg(windows)]
    pub fn child_file_stat(&self, name: &OsStr) -> io::Result<Option<ChildFileStat>> {
        windows::stat_child_file(self.pinned.path(), name)
    }

    #[cfg(not(any(unix, windows)))]
    pub fn child_file_stat(&self, name: &OsStr) -> io::Result<Option<ChildFileStat>> {
        let metadata = match fs::symlink_metadata(self.path.join(name)) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if !metadata.file_type().is_file() {
            return Ok(None);
        }
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let accessed = metadata.accessed().unwrap_or(modified);
        Ok(Some(ChildFileStat { len: metadata.len(), modified, accessed }))
    }

    /// Read the regular file `name` directly inside this directory without
    /// following symbolic links. Returns `None` for directories, symbolic
    /// links, special files, and missing entries.
    #[cfg(unix)]
    pub fn read_child_file(&self, name: &OsStr) -> io::Result<Option<Vec<u8>>> {
        unix::read_file_at(self.handle.as_fd(), &c_name(name)?)
    }

    #[cfg(windows)]
    pub fn read_child_file(&self, name: &OsStr) -> io::Result<Option<Vec<u8>>> {
        windows::read_child_file(&self.pinned, name)
    }

    #[cfg(not(any(unix, windows)))]
    pub fn read_child_file(&self, name: &OsStr) -> io::Result<Option<Vec<u8>>> {
        if self.child_file_stat(name)?.is_none() {
            return Ok(None);
        }
        match fs::read(self.path.join(name)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Whether `name` directly inside this directory is a symbolic link or
    /// reparse point.
    #[cfg(unix)]
    pub fn has_symlink_child(&self, name: &OsStr) -> io::Result<bool> {
        unix::is_symlink_at(self.handle.as_fd(), &c_name(name)?)
    }

    #[cfg(windows)]
    pub fn has_symlink_child(&self, name: &OsStr) -> io::Result<bool> {
        windows::is_reparse_child(self.pinned.path(), name)
    }

    #[cfg(not(any(unix, windows)))]
    pub fn has_symlink_child(&self, name: &OsStr) -> io::Result<bool> {
        match fs::symlink_metadata(self.path.join(name)) {
            Ok(metadata) => Ok(metadata.file_type().is_symlink()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Remove the regular file `name` directly inside this directory without
    /// following symbolic links. Returns `false` when `name` is missing, a
    /// directory, or a symbolic link.
    #[cfg(unix)]
    pub fn remove_child_file(&self, name: &OsStr) -> io::Result<bool> {
        unix::unlink_file_at(self.handle.as_fd(), &c_name(name)?)
    }

    #[cfg(windows)]
    pub fn remove_child_file(&self, name: &OsStr) -> io::Result<bool> {
        windows::remove_child_file(&self.pinned, name)
    }

    #[cfg(not(any(unix, windows)))]
    pub fn remove_child_file(&self, name: &OsStr) -> io::Result<bool> {
        if self.child_file_stat(name)?.is_none() {
            return Ok(false);
        }
        match fs::remove_file(self.path.join(name)) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Remove the empty directory `name` directly inside this directory without
    /// following symbolic links. Returns `false` when `name` is non-empty,
    /// missing, a file, or a symbolic link.
    #[cfg(unix)]
    pub fn remove_empty_child_dir(&self, name: &OsStr) -> io::Result<bool> {
        unix::unlink_empty_dir_at(self.handle.as_fd(), &c_name(name)?)
    }

    #[cfg(windows)]
    pub fn remove_empty_child_dir(&self, name: &OsStr) -> io::Result<bool> {
        windows::remove_empty_child_dir(&self.pinned, name)
    }

    #[cfg(not(any(unix, windows)))]
    pub fn remove_empty_child_dir(&self, name: &OsStr) -> io::Result<bool> {
        let path = self.path.join(name);
        if !is_dir_no_follow(&path)? {
            return Ok(false);
        }
        match fs::remove_dir(&path) {
            Ok(()) => Ok(true),
            Err(error)
                if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::DirectoryNotEmpty) =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(not(unix))]
fn read_dir_entry_names(path: &Path) -> io::Result<Vec<OsString>> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    entries
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect()
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
#[cfg(not(any(unix, windows)))]
fn is_dir_no_follow(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.file_type().is_dir()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests;
