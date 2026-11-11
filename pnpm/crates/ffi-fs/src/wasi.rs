//! The filesystem calls the WASI build makes through `libc` rather than
//! through the supervisor.
//!
//! WASI reaches these through its own preview-1 interface, so they are
//! ordinary `libc` calls there rather than host imports. What `libc` cannot
//! answer on WASI goes through
//! [`pnpm-ffi-webcontainer`](../ffi-webcontainer) instead.

use std::{ffi::CString, fs::File, io, mem::MaybeUninit, os::fd::AsRawFd as _, path::Path};

/// What identifies a file on its filesystem: the device it is on and its
/// inode number there.
///
/// Two paths name one file exactly when their identities are equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIdentity {
    device: libc::dev_t,
    inode: libc::ino_t,
}

/// Create a symlink at `link` pointing at `target`.
pub fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    let target = path_name(target)?;
    let link = path_name(link)?;
    // SAFETY: both strings are NUL-terminated and borrowed for the whole
    // call, which copies the target into the new link and retains neither
    // pointer.
    let linked = unsafe { libc::symlink(target.as_ptr(), link.as_ptr()) };
    if linked == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// The identity of the entry at `path`, without following a symlink at
/// `path` itself.
pub fn path_identity(path: &Path) -> io::Result<FileIdentity> {
    let path = path_name(path)?;
    let mut metadata = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `path` is NUL-terminated and borrowed for the whole call, and
    // the pointer is to a stack local of exactly the type `lstat` writes,
    // which outlives it. The `!= 0` below returns before anything reads an
    // uninitialized field.
    if unsafe { libc::lstat(path.as_ptr(), metadata.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `lstat` reported success, so it initialized the structure.
    let metadata = unsafe { metadata.assume_init() };
    Ok(FileIdentity { device: metadata.st_dev, inode: metadata.st_ino })
}

/// The identity of an already open file.
pub fn file_identity(file: &File) -> io::Result<FileIdentity> {
    let metadata = file_metadata(file)?;
    Ok(FileIdentity { device: metadata.st_dev, inode: metadata.st_ino })
}

/// How many directory entries name an already open file.
pub fn file_link_count(file: &File) -> io::Result<u64> {
    file_metadata(file).map(|metadata| metadata.st_nlink)
}

/// Open the file at `path` for writing, refusing a symlink at `path` itself
/// and without waiting on a reader for a FIFO.
///
/// The file is not created: a missing path fails rather than appearing.
pub fn open_for_overwrite(path: &Path) -> io::Result<File> {
    use std::os::fd::{FromRawFd as _, OwnedFd};

    let path = path_name(path)?;
    // SAFETY: `path` is NUL-terminated and borrowed for the whole call. The
    // flags create nothing, so `open` reads no variadic mode argument and
    // none is passed.
    let descriptor =
        unsafe { libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful `open` returns a descriptor that is new and
    // owned by this call, so no other owner exists to close it.
    let owned = unsafe { OwnedFd::from_raw_fd(descriptor) };
    Ok(File::from(owned))
}

fn file_metadata(file: &File) -> io::Result<libc::stat> {
    let mut metadata = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: the `&File` keeps the descriptor open for the whole call, and
    // the pointer is to a stack local of exactly the type `fstat` writes,
    // which outlives it. The `!= 0` below returns before anything reads an
    // uninitialized field.
    if unsafe { libc::fstat(file.as_raw_fd(), metadata.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fstat` reported success, so it initialized the structure.
    Ok(unsafe { metadata.assume_init() })
}

fn path_name(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}
