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

mod child_ops;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use self::child_ops::unique_child_tmp_name;
#[cfg(windows)]
use self::child_ops::write_child_file_in_pinned_dir;

#[derive(Clone, Copy)]
pub(super) enum ChildWrite {
    CreateNew,
    Replace,
}

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
    pub fn open(path: &Path) -> io::Result<Self> {
        #[cfg(unix)]
        {
            let handle = fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY)
                .open(path)?;
            Ok(Self { path: path.to_path_buf(), handle })
        }
        #[cfg(windows)]
        {
            let pinned = windows::open_root(path)?;
            Ok(Self { path: path.to_path_buf(), pinned })
        }
        #[cfg(not(any(unix, windows)))]
        {
            if fs::metadata(path)?.is_dir() {
                Ok(Self { path: path.to_path_buf() })
            } else {
                Err(io::Error::from(ErrorKind::NotADirectory))
            }
        }
    }

    /// Open the directory `name`, directly inside this one, or `None` when
    /// `name` is not a directory: a file, a symbolic link, or nothing.
    pub fn open_child_dir(&self, name: &OsStr) -> io::Result<Option<Self>> {
        let name = validate_child_name(name)?;
        #[cfg(unix)]
        {
            match unix::open_dir_at(self.handle.as_fd(), &c_name(name)?) {
                Ok(handle) => Ok(Some(Self { path: self.path.join(name), handle: handle.into() })),
                Err(error) if unix::is_not_a_real_dir(&error) => Ok(None),
                Err(error) => Err(error),
            }
        }
        #[cfg(windows)]
        {
            Ok(windows::open_child_dir(&self.path, &self.pinned, name)?
                .map(|(path, pinned)| Self { path, pinned }))
        }
        #[cfg(not(any(unix, windows)))]
        {
            let path = self.path.join(name);
            if is_dir_no_follow(&path)? { Ok(Some(Self { path })) } else { Ok(None) }
        }
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
    pub fn canonical_path(&self) -> io::Result<Option<PathBuf>> {
        let Some(canonical) = canonicalize(&self.path)? else {
            return Ok(None);
        };
        #[cfg(unix)]
        {
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
        {
            Ok((canonical == self.pinned.path() && canonical.is_dir()).then_some(canonical))
        }
        #[cfg(not(any(unix, windows)))]
        {
            Ok(canonical.is_dir().then_some(canonical))
        }
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
    pub fn remove_child_dir(&self, name: &OsStr) -> io::Result<bool> {
        let name = validate_child_name(name)?;
        #[cfg(unix)]
        {
            let c_str = c_name(name)?;
            let parent = self.handle.as_fd();
            match unix::open_dir_at(parent, &c_str) {
                Ok(dir) => unix::remove_dir_all_at(parent, &c_str, dir).map(|()| true),
                Err(error) if unix::is_not_a_real_dir(&error) => Ok(false),
                Err(error) => Err(error),
            }
        }
        #[cfg(windows)]
        {
            windows::remove_child_dir(self, name)
        }
        #[cfg(not(any(unix, windows)))]
        {
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
}

fn validate_child_name(name: &OsStr) -> io::Result<&OsStr> {
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(component)), None)
            if component == name && !name.as_encoded_bytes().contains(&0) =>
        {
            Ok(name)
        }
        _ => Err(io::Error::new(
            ErrorKind::InvalidInput,
            "child name must be one normal path component",
        )),
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

fn canonicalize(path: &Path) -> io::Result<Option<PathBuf>> {
    match dunce::canonicalize(path) {
        Ok(canonical) => Ok(Some(canonical)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn c_name(name: &OsStr) -> io::Result<CString> {
    Ok(CString::new(validate_child_name(name)?.as_bytes())?)
}

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
