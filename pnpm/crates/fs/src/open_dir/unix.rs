//! Opening and removing entries relative to an open directory descriptor,
//! which a link put in the directory's path cannot redirect.

use std::{
    ffi::{CStr, OsStr, OsString},
    io::{self, ErrorKind},
    os::{
        fd::{AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd},
        raw::c_int,
        unix::ffi::OsStrExt,
    },
};

/// Open the directory `name` inside `parent` without following a link.
pub(super) fn open_dir_at(parent: BorrowedFd<'_>, name: &CStr) -> io::Result<OwnedFd> {
    // SAFETY: `name` is NUL-terminated and `parent` stays open for the call.
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the descriptor was just opened and nothing else owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(descriptor) })
}

/// Whether [`open_dir_at`] failed because nothing, a file, or a link is at
/// the name. `O_NOFOLLOW` refuses a link with `ELOOP` on Linux and macOS
/// and with `EMLINK` on FreeBSD.
pub(super) fn is_not_a_real_dir(error: &io::Error) -> bool {
    matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory)
        || matches!(error.raw_os_error(), Some(libc::ELOOP | libc::EMLINK))
}

/// Remove the directory `dir`, opened with [`open_dir_at`] as `name`
/// inside `parent`, with everything in it.
pub(super) fn remove_dir_all_at(
    parent: BorrowedFd<'_>,
    name: &CStr,
    dir: OwnedFd,
) -> io::Result<()> {
    remove_entries(dir)?;
    unlink_at(parent, name, libc::AT_REMOVEDIR)
}

/// Remove the entry `name` inside `parent`: a file or a link is unlinked,
/// a directory is emptied first. A missing entry is already removed.
fn remove_dirent_at(parent: BorrowedFd<'_>, name: &CStr) -> io::Result<()> {
    match open_dir_at(parent, name) {
        Ok(dir) => remove_dir_all_at(parent, name, dir),
        Err(error) if is_not_a_real_dir(&error) => unlink_at(parent, name, 0),
        Err(error) => Err(error),
    }
}

fn unlink_at(parent: BorrowedFd<'_>, name: &CStr, flags: c_int) -> io::Result<()> {
    // SAFETY: `name` is NUL-terminated and `parent` stays open for the call.
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), flags) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.kind() == ErrorKind::NotFound { Ok(()) } else { Err(error) }
}

/// The names of the entries of `dir`, `.` and `..` excluded.
///
/// The stream reads a descriptor opened through `dir` rather than a
/// duplicate of it. A duplicate would share `dir`'s file offset, so two
/// listings at once would consume each other's entries and each return
/// only part of the directory.
pub(super) fn entry_names(dir: BorrowedFd<'_>) -> io::Result<Vec<OsString>> {
    let stream = open_stream(open_dir_at(dir, c".")?)?;
    let result = collect_entry_names(stream);
    close_stream(stream);
    result
}

fn remove_entries(dir: OwnedFd) -> io::Result<()> {
    let stream = open_stream(dir)?;
    let result = remove_stream_entries(stream);
    close_stream(stream);
    result
}

/// Open a directory stream reading `dir`, which the stream takes over.
fn open_stream(dir: OwnedFd) -> io::Result<*mut libc::DIR> {
    // SAFETY: `dir` is an open directory descriptor. On success the stream
    // takes it over and `closedir` releases it; on failure `dir` is dropped.
    let stream = unsafe { libc::fdopendir(dir.as_raw_fd()) };
    if stream.is_null() {
        return Err(io::Error::last_os_error());
    }
    let _ = dir.into_raw_fd();
    Ok(stream)
}

fn close_stream(stream: *mut libc::DIR) {
    // SAFETY: `stream` came from `fdopendir` and is closed once.
    unsafe { libc::closedir(stream) };
}

fn collect_entry_names(stream: *mut libc::DIR) -> io::Result<Vec<OsString>> {
    let mut names = Vec::new();
    for_each_entry(stream, |name| {
        names.push(OsStr::from_bytes(name.to_bytes()).to_os_string());
        Ok(())
    })?;
    Ok(names)
}

fn remove_stream_entries(stream: *mut libc::DIR) -> io::Result<()> {
    // SAFETY: `stream` is open, and the descriptor is borrowed only while
    // it is.
    let dir = unsafe { BorrowedFd::borrow_raw(libc::dirfd(stream)) };
    for_each_entry(stream, |name| remove_dirent_at(dir, name))
}

/// Call `visit` with the name of each entry of `stream`, `.` and `..`
/// excluded. A name is valid only until the next entry is read.
fn for_each_entry(
    stream: *mut libc::DIR,
    mut visit: impl FnMut(&CStr) -> io::Result<()>,
) -> io::Result<()> {
    loop {
        clear_errno();
        // SAFETY: `stream` is open. The entry is read before the next
        // `readdir` call, which may reuse it.
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(0) { Ok(()) } else { Err(error) };
        }
        // SAFETY: the C library NUL-terminates `d_name`.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if matches!(name.to_bytes(), b"." | b"..") {
            continue;
        }
        visit(name)?;
    }
}

/// `readdir` reports the end of a directory and an error the same way, so
/// `errno` has to be zero before the call to tell them apart.
fn clear_errno() {
    // SAFETY: the pointer is the calling thread's `errno`.
    unsafe { *errno_location() = 0 }
}

#[cfg(any(target_os = "linux", target_os = "emscripten", target_os = "redox"))]
fn errno_location() -> *mut c_int {
    // SAFETY: the function takes no arguments and always returns a valid pointer.
    unsafe { libc::__errno_location() }
}

#[cfg(any(target_os = "android", target_os = "netbsd", target_os = "openbsd"))]
fn errno_location() -> *mut c_int {
    // SAFETY: the function takes no arguments and always returns a valid pointer.
    unsafe { libc::__errno() }
}

#[cfg(any(target_vendor = "apple", target_os = "freebsd", target_os = "dragonfly"))]
fn errno_location() -> *mut c_int {
    // SAFETY: the function takes no arguments and always returns a valid pointer.
    unsafe { libc::__error() }
}
