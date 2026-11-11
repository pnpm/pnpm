//! The supervisor's filesystem imports: the operations the WASI
//! preview-1 interface does not reach.
//!
//! Every path is made absolute before it is handed over, because the
//! supervisor resolves one against its own working directory rather than
//! the guest's.

use pnpm_webcontainer_sys::pnpm_fs as sys;
use std::{fs::File, io, path::Path};

/// Whether a lock request excludes every other holder or only the writers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exclusivity {
    /// Excludes every other lease on the file.
    Exclusive,
    /// Excludes only exclusive leases, so readers hold it together.
    Shared,
}

/// Exclusively create a file at `path` with the requested host permission
/// bits.
pub fn create_new(path: &Path, mode: u32) -> io::Result<File> {
    with_absolute_path(path, |path| {
        let mut descriptor = 0;
        // SAFETY: the slice is borrowed for the whole synchronous call,
        // which copies what it needs and retains no pointer, and the output
        // is a fully initialized stack local that outlives the call.
        check(unsafe { sys::create_new(path.as_ptr(), path.len(), mode, &raw mut descriptor) })?;
        adopt(descriptor)
    })
}

/// Open the file at `path` without following a symlink at `path` itself,
/// and without waiting on a writer for a FIFO.
pub fn open_nofollow(path: &Path) -> io::Result<File> {
    with_absolute_path(path, |path| {
        let mut descriptor = 0;
        // SAFETY: as in `create_new`.
        check(unsafe { sys::open_nofollow(path.as_ptr(), path.len(), &raw mut descriptor) })?;
        adopt(descriptor)
    })
}

/// Open the directory at `path`, refusing a symlink at every component
/// below `template`. `template` itself may be a symlink.
///
/// The host's path walk and its final open are separate operations because
/// the supervisor offers no descriptor-stable `openat`.
pub fn open_directory_nofollow_beneath(path: &Path, template: &Path) -> io::Result<File> {
    with_absolute_path(path, |path| {
        with_absolute_path(template, |template| {
            let mut descriptor = 0;
            // SAFETY: both slices are borrowed for the whole synchronous
            // call, which retains neither pointer, and the output is a
            // fully initialized stack local that outlives it.
            check(unsafe {
                sys::open_directory_nofollow_beneath(
                    path.as_ptr(),
                    path.len(),
                    template.as_ptr(),
                    template.len(),
                    &raw mut descriptor,
                )
            })?;
            adopt(descriptor)
        })
    })
}

/// Add `extra` to the mode of every directory from `path` up to, but not
/// including, `template`.
///
/// Fails with [`io::ErrorKind::Unsupported`] on a host that cannot resolve
/// `/proc/self/fd`, which is the signal to fall back to the caller's own
/// WASI descriptor walk.
pub fn grant_directory_mode_beneath(path: &Path, template: &Path, extra: u32) -> io::Result<()> {
    with_absolute_path(path, |path| {
        with_absolute_path(template, |template| {
            // SAFETY: both slices are borrowed for the whole synchronous
            // call, which retains neither pointer, and `extra` is taken by
            // value.
            check(unsafe {
                sys::grant_directory_mode_beneath(
                    path.as_ptr(),
                    path.len(),
                    template.as_ptr(),
                    template.len(),
                    extra,
                )
            })
        })
    })
}

/// Open the lock file at `path`, creating it if it is not there.
pub fn open_lock(path: &Path) -> io::Result<File> {
    with_absolute_path(path, |path| {
        let mut descriptor = 0;
        // SAFETY: as in `create_new`.
        check(unsafe { sys::open_lock(path.as_ptr(), path.len(), &raw mut descriptor) })?;
        adopt(descriptor)
    })
}

/// Try to hold a host lease on `file` until every handle to it is closed,
/// and report the host's `errno`: `0` for success, `EAGAIN` for a lease
/// another holder has.
///
/// The `errno` is returned rather than an [`io::Error`] because the one
/// caller branches on `EAGAIN` before it has an error to build.
#[must_use]
pub fn try_lock(file: &File, exclusivity: Exclusivity) -> i32 {
    use std::os::fd::AsRawFd as _;

    let exclusive = match exclusivity {
        Exclusivity::Exclusive => 1,
        Exclusivity::Shared => 0,
    };
    // SAFETY: the `&File` keeps the descriptor open for the whole call, and
    // the host takes both arguments by value.
    unsafe { sys::try_lock(file.as_raw_fd(), exclusive) }
}

/// Apply the database's permissions atomically as its `SQLite` sidecar
/// files are created, or stop doing so.
pub fn register_sqlite_permissions(path: &Path, registered: bool) -> io::Result<()> {
    with_absolute_path(path, |path| {
        // SAFETY: the slice is borrowed for the whole synchronous call,
        // which retains no pointer, and the flag is taken by value.
        check(unsafe { sys::sqlite_register(path.as_ptr(), path.len(), u32::from(registered)) })
    })
}

/// Check that the current user owns `file`.
///
/// A store shared across users cannot coordinate, because each user's
/// leases live in a namespace of their own.
pub fn check_file_owner(file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd as _;

    // SAFETY: the `&File` keeps the descriptor open for the whole call,
    // which takes it by value.
    check(unsafe { sys::check_owner(file.as_raw_fd()) })
}

/// Check execute access to `path` using the supervising process's
/// credentials rather than the guest's.
pub fn executable_access(path: &Path) -> io::Result<()> {
    with_absolute_path(path, |path| {
        // SAFETY: the slice is borrowed for the whole synchronous call,
        // which retains no pointer.
        check(unsafe { sys::path_access_executable(path.as_ptr(), path.len()) })
    })
}

/// Set `file`'s mode.
pub fn set_file_mode(file: &File, mode: u32) -> io::Result<()> {
    use std::os::fd::AsRawFd as _;

    // SAFETY: the `&File` keeps the descriptor open for the whole call,
    // which takes both arguments by value.
    check(unsafe { sys::fchmod(file.as_raw_fd(), mode) })
}

/// `file`'s mode.
pub fn file_mode(file: &File) -> io::Result<u32> {
    use std::os::fd::AsRawFd as _;

    let mut mode = 0;
    // SAFETY: the `&File` keeps the descriptor open for the whole call, and
    // the output is a fully initialized stack local that outlives it.
    check(unsafe { sys::fmode(file.as_raw_fd(), &raw mut mode) })?;
    Ok(mode)
}

/// The mode of the entry at `path`, without following a symlink at `path`
/// itself.
pub fn path_mode(path: &Path) -> io::Result<u32> {
    with_absolute_path(path, |path| {
        let mut mode = 0;
        // SAFETY: the slice is borrowed and the output is a fully
        // initialized stack local, both for the whole synchronous call,
        // which retains neither pointer.
        check(unsafe { sys::lmode(path.as_ptr(), path.len(), &raw mut mode) })?;
        Ok(mode)
    })
}

/// Create the directory at `path` and every missing parent, with
/// permissions only its owner can use.
pub fn secure_directory(path: &Path) -> io::Result<()> {
    with_absolute_path(path, |path| {
        // SAFETY: the slice is borrowed for the whole synchronous call,
        // which retains no pointer.
        check(unsafe { sys::secure_directory(path.as_ptr(), path.len()) })
    })
}

/// The supervising process's user id.
#[must_use]
pub fn user_id() -> u32 {
    // SAFETY: the import reads the host's user identity. It takes no
    // arguments, touches no guest memory, and cannot fail.
    unsafe { sys::uid() }
}

/// The supervising process's file-creation mask.
#[must_use]
pub fn current_umask() -> u32 {
    // SAFETY: the import reads the host's mask. It takes no arguments,
    // touches no guest memory, and cannot fail.
    unsafe { sys::umask() }
}

/// Adopt the descriptor a successful host open transferred, so that it is
/// closed exactly once, by the returned `File`.
fn adopt(descriptor: u32) -> io::Result<File> {
    use std::os::fd::FromRawFd as _;

    let descriptor =
        descriptor.try_into().map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
    // SAFETY: the host reported success, which transfers ownership of this
    // WASI descriptor to the guest, so no other owner exists to close it.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

fn with_absolute_path<Value>(
    path: &Path,
    operation: impl FnOnce(&[u8]) -> io::Result<Value>,
) -> io::Result<Value> {
    let absolute = std::path::absolute(path)?;
    operation(absolute.as_os_str().as_encoded_bytes())
}

fn check(errno: i32) -> io::Result<()> {
    if errno == 0 { Ok(()) } else { Err(io::Error::from_raw_os_error(errno)) }
}
