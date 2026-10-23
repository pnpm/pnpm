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
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
};

/// A directory held open while its children are removed.
///
/// On Unix a child is reached through the open directory, so a symbolic
/// link put in the directory's place after the open is not followed. On
/// other platforms every operation resolves the directory's path again.
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
    pub fn open(path: &Path) -> io::Result<Self> {
        open_dir(path, Follow::Links)
    }

    /// Open the directory at `path`, or `None` when nothing is there or
    /// it is not a directory, a symbolic link to one included.
    pub fn open_no_follow(path: &Path) -> io::Result<Option<Self>> {
        match open_dir(path, Follow::Nothing) {
            Ok(dir) => Ok(Some(dir)),
            Err(error) if is_not_a_real_dir(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// The path the directory was opened at.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The absolute path of this directory with every symbolic link
    /// resolved, or `None` when its path no longer leads to it.
    pub fn canonical_path(&self) -> io::Result<Option<PathBuf>> {
        let canonical = match dunce::canonicalize(&self.path) {
            Ok(canonical) => canonical,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if self.is_at(&canonical)? { Ok(Some(canonical)) } else { Ok(None) }
    }

    /// Remove the directory `name`, directly inside this one, with
    /// everything in it. Returns `false` and leaves the entry in place when
    /// `name` is not a directory: a file, a symbolic link, or nothing.
    ///
    /// On Unix the directory is first renamed, through the open directory,
    /// to a name only this process knows. The removal then resolves this
    /// directory's path again, and a directory substituted in its place
    /// cannot hold that name, so the renamed directory stays behind in the
    /// opened one. The new name keeps `name` as its prefix.
    pub fn remove_child_dir(&self, name: &OsStr) -> io::Result<bool> {
        let Some(removal_name) = self.detach_child_dir(name)? else {
            return Ok(false);
        };
        match remove_dirent(&self.path.join(removal_name)) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
            Err(error) => Err(error),
        }
    }

    #[cfg(unix)]
    fn is_at(&self, path: &Path) -> io::Result<bool> {
        let opened = self.handle.metadata()?;
        let named = match fs::metadata(path) {
            Ok(named) => named,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        Ok(opened.dev() == named.dev() && opened.ino() == named.ino())
    }

    #[cfg(not(unix))]
    fn is_at(&self, path: &Path) -> io::Result<bool> {
        Ok(path.is_dir())
    }

    /// Rename the directory `name` to a name only this process knows and
    /// return that name, or `None` when `name` is not a directory.
    #[cfg(unix)]
    fn detach_child_dir(&self, name: &OsStr) -> io::Result<Option<PathBuf>> {
        let child = CString::new(name.as_bytes())?;
        if !self.has_child_dir(&child)? {
            return Ok(None);
        }
        let mut removal_name = name.to_os_string();
        removal_name.push(".");
        removal_name.push(crate::random_hex::random_hex()?);
        let removal = CString::new(removal_name.as_bytes())?;
        let descriptor = self.handle.as_raw_fd();
        // SAFETY: both names are NUL-terminated and the directory stays open
        // for the call.
        if unsafe { libc::renameat(descriptor, child.as_ptr(), descriptor, removal.as_ptr()) } == 0
        {
            return Ok(Some(PathBuf::from(removal_name)));
        }
        let error = io::Error::last_os_error();
        if error.kind() == ErrorKind::NotFound { Ok(None) } else { Err(error) }
    }

    #[cfg(unix)]
    fn has_child_dir(&self, name: &CString) -> io::Result<bool> {
        // SAFETY: `name` is NUL-terminated and the directory stays open for
        // the call.
        let descriptor = unsafe {
            libc::openat(
                self.handle.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor >= 0 {
            // SAFETY: the descriptor was just opened and nothing else owns it.
            drop(unsafe { fs::File::from_raw_fd(descriptor) });
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if is_not_a_real_dir(&error) { Ok(false) } else { Err(error) }
    }

    #[cfg(not(unix))]
    fn detach_child_dir(&self, name: &OsStr) -> io::Result<Option<PathBuf>> {
        match fs::symlink_metadata(self.path.join(name)) {
            Ok(metadata) if metadata.file_type().is_dir() => Ok(Some(PathBuf::from(name))),
            Ok(_) => Ok(None),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
}

#[derive(Clone, Copy)]
enum Follow {
    Links,
    Nothing,
}

#[cfg(unix)]
fn open_dir(path: &Path, follow: Follow) -> io::Result<OpenDir> {
    let flags = match follow {
        Follow::Links => libc::O_DIRECTORY,
        Follow::Nothing => libc::O_DIRECTORY | libc::O_NOFOLLOW,
    };
    let handle = fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(path)?;
    Ok(OpenDir { path: path.to_path_buf(), handle })
}

#[cfg(not(unix))]
fn open_dir(path: &Path, follow: Follow) -> io::Result<OpenDir> {
    let metadata = match follow {
        Follow::Links => fs::metadata(path)?,
        Follow::Nothing => fs::symlink_metadata(path)?,
    };
    if metadata.file_type().is_dir() {
        Ok(OpenDir { path: path.to_path_buf() })
    } else {
        Err(io::Error::from(ErrorKind::NotADirectory))
    }
}

/// Whether opening a directory without following links failed because
/// nothing, a file, or a link is at the path.
fn is_not_a_real_dir(error: &io::Error) -> bool {
    matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) || is_link_refusal(error)
}

/// Whether `O_NOFOLLOW` refused a link: `ELOOP` on Linux and macOS,
/// `EMLINK` on FreeBSD.
#[cfg(unix)]
fn is_link_refusal(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(libc::ELOOP | libc::EMLINK))
}

#[cfg(not(unix))]
fn is_link_refusal(_error: &io::Error) -> bool {
    false
}

#[cfg(test)]
mod tests;
