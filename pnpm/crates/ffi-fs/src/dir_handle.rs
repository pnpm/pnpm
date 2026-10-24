//! Operations taken against an open directory handle rather than a path.
//!
//! A handle pins the directory it was opened on, so a sequence of these
//! cannot be redirected part-way through by something that replaces a
//! component with a symlink. Every entry name is resolved in the handle's
//! directory and nowhere else: a name is a single component as far as these
//! calls are concerned, and `O_NOFOLLOW` refuses a symlink at the name
//! itself.

use super::FileMode;
use std::{
    ffi::{CString, OsStr, OsString},
    fs::File,
    io,
    os::{
        fd::{AsRawFd as _, FromRawFd as _, OwnedFd},
        unix::ffi::{OsStrExt as _, OsStringExt as _},
    },
    path::{Path, PathBuf},
};

/// Whether a directory handle can be read or only traversed.
///
/// A directory whose mode denies reading can still be opened for traversal
/// alone, which is enough to reach its entries by name but not to list
/// them or to read its metadata through the handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryAccess {
    /// The handle can be read: its entries can be listed.
    Readable,
    /// The handle can only be traversed: entries are reachable by name.
    SearchOnly,
}

const DIRECTORY_FLAGS: libc::c_int =
    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;

/// Open the directory `name` inside `parent`, refusing a symlink at `name`.
pub fn open_directory_at(parent: &File, name: &OsStr) -> io::Result<File> {
    let name = entry_name(name)?;
    adopt(open_at(parent, &name, DIRECTORY_FLAGS, None))
}

/// Open the directory `name` inside `parent` for traversal, falling back to
/// a search-only handle for a directory whose mode denies reading it, and
/// report which of the two was opened.
pub fn open_directory_for_traversal(
    parent: &File,
    name: &OsStr,
) -> io::Result<(File, DirectoryAccess)> {
    let name = entry_name(name)?;
    let readable = open_at(parent, &name, DIRECTORY_FLAGS, None);
    if readable >= 0 {
        return Ok((adopt(readable)?, DirectoryAccess::Readable));
    }
    if io::Error::last_os_error().kind() != io::ErrorKind::PermissionDenied {
        return Err(io::Error::last_os_error());
    }
    let search = open_at(parent, &name, search_only_flags(), None);
    Ok((adopt(search)?, DirectoryAccess::SearchOnly))
}

/// Open the directory at `path` for traversal, following a symlink at
/// `path` itself.
///
/// For the anchor of a walk, where the caller has chosen the path and a
/// symlink at it is theirs to follow. Every step below the anchor goes
/// through [`open_directory_for_traversal`], which refuses one.
pub fn open_search_directory(path: &Path) -> io::Result<File> {
    let path = path_argument(path.as_os_str())?;
    let flags = search_only_flags() & !libc::O_NOFOLLOW;
    // SAFETY: `path` is NUL-terminated and borrowed for the whole call. The
    // flags create nothing, so `open` reads no variadic mode argument and
    // none is passed.
    adopt(unsafe { libc::open(path.as_ptr(), flags) })
}

/// Create the directory `name` inside `parent` at `mode`, before the umask
/// is applied to it.
pub fn create_directory_at(parent: &File, name: &OsStr, mode: FileMode) -> io::Result<()> {
    let name = entry_name(name)?;
    let mode = mode.narrow();
    // SAFETY: `name` is NUL-terminated and borrowed for the whole call, and
    // the `&File` keeps the parent descriptor open across it. `mkdirat`
    // reads the name, writes nothing through a pointer, and retains
    // neither.
    let created = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), mode) };
    if created == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// Open the file `name` inside `parent` for reading, refusing a symlink at
/// `name`.
pub fn open_file_at(parent: &File, name: &OsStr) -> io::Result<File> {
    let name = entry_name(name)?;
    adopt(open_at(parent, &name, libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC, None))
}

/// Open the file `name` inside `parent` for reading without waiting on a
/// writer, so that a FIFO or a device at `name` fails on its type instead
/// of blocking.
pub fn open_file_at_without_blocking(parent: &File, name: &OsStr) -> io::Result<File> {
    let name = entry_name(name)?;
    let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
    adopt(open_at(parent, &name, flags, None))
}

/// Create the file `name` inside `parent` at `mode`, failing if anything
/// already occupies the name.
pub fn create_file_at(parent: &File, name: &OsStr, mode: FileMode) -> io::Result<File> {
    let name = entry_name(name)?;
    let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    adopt(open_at(parent, &name, flags, Some(mode.narrow())))
}

/// Rename `from` to `to`, both inside `parent`, replacing whatever `to`
/// names.
pub fn rename_at(parent: &File, from: &OsStr, to: &OsStr) -> io::Result<()> {
    let from = entry_name(from)?;
    let to = entry_name(to)?;
    // SAFETY: both names are NUL-terminated and borrowed for the whole
    // call, and the `&File` keeps the descriptor open across it. Passing
    // the same descriptor for both directory arguments names one directory
    // for both halves of the rename, which is what an entry-level rename
    // is.
    let renamed = unsafe {
        libc::renameat(parent.as_raw_fd(), from.as_ptr(), parent.as_raw_fd(), to.as_ptr())
    };
    if renamed == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// Remove the entry `name` inside `parent`. A directory at `name` is not
/// removed: the call fails with a platform-dependent error.
pub fn unlink_at(parent: &File, name: &OsStr) -> io::Result<()> {
    let name = entry_name(name)?;
    // SAFETY: `name` is NUL-terminated and borrowed for the whole call, and
    // the `&File` keeps the parent descriptor open across it. A zero flag
    // word is the entry-removal form, which is the one documented to refuse
    // a directory.
    let unlinked = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) };
    if unlinked == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// Remove the directory `name` inside `parent`, which has to be empty.
///
/// The counterpart of [`unlink_at`], which removes everything else: the two
/// flag words `unlinkat` takes are the only difference between them.
pub fn remove_directory_at(parent: &File, name: &OsStr) -> io::Result<()> {
    let name = entry_name(name)?;
    // SAFETY: `name` is NUL-terminated and borrowed for the whole call, and
    // the `&File` keeps the parent descriptor open across it. `AT_REMOVEDIR`
    // is the directory-removal form, which refuses anything else.
    let removed = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) };
    if removed == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// Create a symlink at `name` inside `parent`, pointing at `target`.
///
/// `target` is stored as given, so a relative one resolves against
/// `parent` when the link is followed.
pub fn symlink_at(target: &Path, parent: &File, name: &OsStr) -> io::Result<()> {
    let target = path_argument(target.as_os_str())?;
    let name = entry_name(name)?;
    // SAFETY: both strings are NUL-terminated and borrowed for the whole
    // call, and the `&File` keeps the parent descriptor open across it.
    // `symlinkat` copies the target into the new link and retains no
    // pointer.
    let linked = unsafe { libc::symlinkat(target.as_ptr(), parent.as_raw_fd(), name.as_ptr()) };
    if linked == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// Read the target of the symlink `name` inside `parent`.
///
/// Fails with [`io::ErrorKind::InvalidInput`] when `name` is not a symlink,
/// which is how `readlinkat` reports a plain file or directory.
pub fn read_link_at(parent: &File, name: &OsStr) -> io::Result<PathBuf> {
    let name = entry_name(name)?;
    let mut capacity = 256;
    loop {
        let mut target = vec![0_u8; capacity];
        // SAFETY: `name` is NUL-terminated and borrowed for the whole call,
        // the `&File` keeps the parent descriptor open across it, and the
        // buffer is a live `Vec` of exactly `target.len()` initialized
        // bytes, which is the length the call is told it may write.
        let written = unsafe {
            libc::readlinkat(
                parent.as_raw_fd(),
                name.as_ptr(),
                target.as_mut_ptr().cast(),
                target.len(),
            )
        };
        if written < 0 {
            return Err(io::Error::last_os_error());
        }
        let written = usize::try_from(written).expect("a nonnegative readlinkat length");
        // A target that filled the buffer may have been truncated, so the
        // only length that is known complete is one short of it.
        if written < target.len() {
            target.truncate(written);
            return Ok(OsString::from_vec(target).into());
        }
        capacity = capacity
            .checked_mul(2)
            .ok_or_else(|| io::Error::other("symlink target exceeds addressable memory"))?;
    }
}

/// Set the mode of the entry `name` inside `parent`, without following a
/// symlink at `name`.
///
/// Linux has no `fchmodat` that refuses to follow the final symlink, so
/// there a caller changes the mode through the handle it already holds.
#[cfg(not(target_os = "linux"))]
pub fn set_mode_at(parent: &File, name: &OsStr, mode: FileMode) -> io::Result<()> {
    let name = entry_name(name)?;
    let mode = mode.narrow();
    // SAFETY: `name` is NUL-terminated and borrowed for the whole call, and
    // the `&File` keeps the parent descriptor open across it. `fchmodat`
    // takes the mode and the flag word by value.
    let changed = unsafe {
        libc::fchmodat(parent.as_raw_fd(), name.as_ptr(), mode, libc::AT_SYMLINK_NOFOLLOW)
    };
    if changed == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// `openat`'s variadic mode argument is read only for a flag set that
/// creates, so it is passed only then: supplying it otherwise would read
/// a register the call does not set.
fn open_at(
    parent: &File,
    name: &CString,
    flags: libc::c_int,
    mode: Option<libc::mode_t>,
) -> libc::c_int {
    match mode {
        // SAFETY: `name` is NUL-terminated and borrowed for the whole call,
        // and the `&File` keeps the parent descriptor open across it. The
        // flags include `O_CREAT`, which is the case in which `openat`
        // reads its variadic mode argument, so one is supplied.
        Some(mode) => unsafe {
            libc::openat(parent.as_raw_fd(), name.as_ptr(), flags, libc::c_uint::from(mode))
        },
        // SAFETY: as above, and the flags create nothing, so `openat` reads
        // no variadic argument and none is passed.
        None => unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) },
    }
}

/// Adopt the descriptor an `open` reported, so that it is closed exactly
/// once, by the returned `File`.
fn adopt(descriptor: libc::c_int) -> io::Result<File> {
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful `openat` returns a descriptor that is new and
    // owned by this call, so no other owner exists to close it, and nothing
    // between the call and here could have closed it.
    let owned = unsafe { OwnedFd::from_raw_fd(descriptor) };
    Ok(File::from(owned))
}

/// The flags for a directory that can be traversed but not read. `O_PATH`
/// and `O_SEARCH` are the respective platforms' spelling of it; elsewhere
/// `O_RDONLY` is the only form, and the open fails where it would have
/// needed the weaker access.
fn search_only_flags() -> libc::c_int {
    #[cfg(target_os = "linux")]
    let access = libc::O_PATH;
    #[cfg(target_os = "macos")]
    let access = libc::O_SEARCH;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let access = libc::O_RDONLY;
    access | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC
}

/// An entry name as a C string: one path component, which is what the
/// handle's directory resolves.
///
/// A name holding a separator resolves below the directory rather than in
/// it, `.` and `..` resolve elsewhere again, and an absolute name ignores
/// the handle altogether. Refusing all three is what makes the module's
/// promise hold for a name the caller did not build itself.
fn entry_name(name: &OsStr) -> io::Result<CString> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes == b"." || bytes == b".." || bytes.contains(&b'/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "an entry name must be a single path component",
        ));
    }
    path_argument(name)
}

/// A path as a C string, rejecting an interior NUL, which no filesystem can
/// store and which would otherwise truncate the path.
fn path_argument(path: &OsStr) -> io::Result<CString> {
    CString::new(path.as_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}

#[cfg(test)]
mod tests;
