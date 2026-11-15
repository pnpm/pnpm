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
    mem::MaybeUninit,
    os::{
        fd::{AsRawFd as _, FromRawFd as _, OwnedFd},
        unix::ffi::{OsStrExt as _, OsStringExt as _},
    },
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
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

/// Which kind of entry a name holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
    /// A symlink, which [`entry_metadata_at`] reports without following.
    Symlink,
    /// A socket, FIFO, device, or anything else a mode can name.
    Other,
}

/// What an entry's own metadata reports.
///
/// The file type arrives as [`EntryKind`] rather than in the mode's high
/// bits, so a caller never masks one off the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryMetadata {
    /// The length the entry reports, which is meaningful for a file.
    pub len: u64,
    /// When the entry was last written.
    pub modified: SystemTime,
    /// When the entry was last read.
    pub accessed: SystemTime,
    /// What the entry is.
    pub kind: EntryKind,
}

const DIRECTORY_FLAGS: libc::c_int =
    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;

/// Open the directory `name` inside `parent`, refusing a symlink at `name`.
pub fn open_directory_at(parent: &File, name: &OsStr) -> io::Result<File> {
    let name = entry_name(name)?;
    adopt(open_at(parent, &name, DIRECTORY_FLAGS, None))
}

/// Open `directory` again through itself, for a handle whose read position
/// is its own.
///
/// `openat` resolves `.` from the directory a descriptor refers to rather
/// than by walking its path, so the new handle is on the same directory
/// even once a link stands in that path.
pub fn reopen_directory(directory: &File) -> io::Result<File> {
    let name = path_argument(OsStr::new("."))?;
    adopt(open_at(directory, &name, DIRECTORY_FLAGS, None))
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
/// Link the entry `from` to the new name `to`, both inside `parent`.
///
/// The counterpart of [`rename_at`], which moves a name: this one adds a
/// second name for the same file and fails if `to` is already taken, so a
/// caller that must not clobber publishes through it.
pub fn link_at(parent: &File, from: &OsStr, to: &OsStr) -> io::Result<()> {
    let from = entry_name(from)?;
    let to = entry_name(to)?;
    // SAFETY: both names are NUL-terminated and borrowed for the whole
    // call, and the `&File` keeps the descriptor open across it. Passing
    // the same descriptor for both directory arguments names one directory
    // for both halves of the link, and a zero flag word does not follow a
    // symlink at `from`.
    let linked = unsafe {
        libc::linkat(parent.as_raw_fd(), from.as_ptr(), parent.as_raw_fd(), to.as_ptr(), 0)
    };
    if linked == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// What the entry `name` inside `parent` reports about itself, or `None`
/// when nothing is there.
///
/// A symlink is reported as one rather than followed, so the answer
/// describes the name and not whatever it points at.
pub fn entry_metadata_at(parent: &File, name: &OsStr) -> io::Result<Option<EntryMetadata>> {
    let name = entry_name(name)?;
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `name` is NUL-terminated and borrowed for the whole call, the
    // `&File` keeps the parent descriptor open across it, and the pointer
    // is to a stack local of exactly the type the call writes.
    // `AT_SYMLINK_NOFOLLOW` reports a symlink itself.
    let reported = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if reported != 0 {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::NotFound { Ok(None) } else { Err(error) };
    }
    // SAFETY: the call reported success, which it only does after writing
    // the whole structure.
    let stat = unsafe { stat.assume_init() };
    // NetBSD is the only Unix whose `stat` spells these `st_*timensec`.
    #[cfg(target_os = "netbsd")]
    let (modified_nanoseconds, accessed_nanoseconds) = (stat.st_mtimensec, stat.st_atimensec);
    #[cfg(not(target_os = "netbsd"))]
    let (modified_nanoseconds, accessed_nanoseconds) = (stat.st_mtime_nsec, stat.st_atime_nsec);
    Ok(Some(EntryMetadata {
        len: u64::try_from(stat.st_size).unwrap_or(0),
        modified: entry_time(stat.st_mtime, modified_nanoseconds),
        accessed: entry_time(stat.st_atime, accessed_nanoseconds),
        kind: entry_kind(stat.st_mode),
    }))
}

fn entry_kind(mode: libc::mode_t) -> EntryKind {
    match mode & libc::S_IFMT {
        libc::S_IFREG => EntryKind::File,
        libc::S_IFDIR => EntryKind::Directory,
        libc::S_IFLNK => EntryKind::Symlink,
        _ => EntryKind::Other,
    }
}

/// A timestamp as the entry reports it, clamped to what [`SystemTime`] can
/// name: a time before the epoch, which only a corrupt or hand-written
/// inode carries, reads as the epoch rather than failing the whole call.
fn entry_time(seconds: libc::time_t, nanoseconds: libc::c_long) -> SystemTime {
    let Ok(seconds) = u64::try_from(seconds) else {
        return SystemTime::UNIX_EPOCH;
    };
    let nanoseconds = u32::try_from(nanoseconds).unwrap_or(0);
    SystemTime::UNIX_EPOCH
        .checked_add(Duration::new(seconds, nanoseconds))
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Whether the calling user may write to and search `path`.
///
/// Asked of the effective user, which is who a write would be permitted
/// for, rather than the real one that comparing a `stat` against an id
/// would describe. A path that does not exist, or that cannot be reached,
/// answers `false`.
pub fn can_write_and_search(path: &Path) -> bool {
    let Ok(path) = path_argument(path.as_os_str()) else {
        return false;
    };
    // SAFETY: the string is NUL-terminated and borrowed for the whole call,
    // which writes no memory. `AT_FDCWD` resolves the path as an ordinary
    // relative or absolute path, and `AT_EACCESS` asks about the effective
    // user rather than the real one.
    let permitted = unsafe {
        libc::faccessat(libc::AT_FDCWD, path.as_ptr(), libc::W_OK | libc::X_OK, libc::AT_EACCESS)
    };
    permitted == 0
}

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
