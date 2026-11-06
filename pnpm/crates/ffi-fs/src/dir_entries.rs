//! Reading the names in an open directory, without naming its path again.
//!
//! [`std::fs::read_dir`] takes a path, which a link put in the directory's
//! place after the handle was opened would redirect. This reads the handle
//! itself.

use std::{
    ffi::{CStr, OsStr, OsString},
    fs::File,
    io,
    os::{
        fd::{AsRawFd as _, IntoRawFd as _},
        raw::c_int,
        unix::ffi::OsStrExt as _,
    },
};

/// The names in `directory`, without `.` and `..`.
///
/// Read in one pass and returned owned, so the caller may change the
/// directory afterwards. Reading and removing at the same time leaves it
/// unspecified whether an entry removed after the read began is reported.
///
/// The stream reads a descriptor opened through `directory` rather than a
/// duplicate of it. A duplicate would share `directory`'s read position, so
/// two listings at once would consume each other's entries and each return
/// part of the directory.
pub fn directory_entry_names(directory: &File) -> io::Result<Vec<OsString>> {
    // `fdopendir` takes the descriptor over and `closedir` releases it, so
    // the stream gets its own and the caller keeps their handle.
    let own = crate::dir_handle::reopen_directory(directory)?;
    // SAFETY: the descriptor is an open directory, and it is released below
    // by `closedir` rather than by `own`, whose ownership is given up.
    let stream = unsafe { libc::fdopendir(own.as_raw_fd()) };
    if stream.is_null() {
        return Err(io::Error::last_os_error());
    }
    let _ = own.into_raw_fd();
    let names = read_names(stream);
    // SAFETY: `stream` came from `fdopendir` above and is closed once.
    unsafe { libc::closedir(stream) };
    names
}

fn read_names(stream: *mut libc::DIR) -> io::Result<Vec<OsString>> {
    let mut names = Vec::new();
    loop {
        clear_errno();
        // SAFETY: `stream` is open, and the entry it returns is read before
        // the next call, which may reuse the storage.
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(0) { Ok(names) } else { Err(error) };
        }
        // SAFETY: `entry` is non-null and the C library NUL-terminates
        // `d_name`.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if matches!(name.to_bytes(), b"." | b"..") {
            continue;
        }
        names.push(OsStr::from_bytes(name.to_bytes()).to_owned());
    }
}

/// `readdir` reports the end of a directory and a failure the same way, so
/// `errno` has to be zero before the call to tell them apart.
fn clear_errno() {
    // SAFETY: the pointer is the calling thread's own `errno`.
    unsafe { *errno_location() = 0 }
}

#[cfg(any(target_os = "linux", target_os = "emscripten", target_os = "redox"))]
fn errno_location() -> *mut c_int {
    // SAFETY: the call takes no argument and returns the thread's `errno`.
    unsafe { libc::__errno_location() }
}

#[cfg(any(target_os = "android", target_os = "netbsd", target_os = "openbsd"))]
fn errno_location() -> *mut c_int {
    // SAFETY: the call takes no argument and returns the thread's `errno`.
    unsafe { libc::__errno() }
}

#[cfg(any(target_vendor = "apple", target_os = "freebsd", target_os = "dragonfly"))]
fn errno_location() -> *mut c_int {
    // SAFETY: the call takes no argument and returns the thread's `errno`.
    unsafe { libc::__error() }
}

#[cfg(test)]
mod tests;
