use super::{is_symlink_at, unlink_at};
use crate::open_dir::{ChildWrite, unique_child_tmp_name};
use std::{
    ffi::{CStr, CString, OsStr},
    fs::File,
    io::{self, ErrorKind, Write as _},
    os::{
        fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd},
        unix::ffi::OsStrExt,
    },
};

const MAX_TEMP_ATTEMPTS: usize = 16;

pub(in crate::open_dir) fn write_file_at(
    parent: BorrowedFd<'_>,
    base_name: &OsStr,
    dest_name: &CStr,
    bytes: &[u8],
    mode: ChildWrite,
) -> io::Result<()> {
    match mode {
        ChildWrite::CreateNew => {}
        ChildWrite::Replace => {
            if is_symlink_at(parent, dest_name)? {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    "refusing to replace symlink child",
                ));
            }
        }
    }
    let (mut file, tmp_name) = create_temp_at(parent, base_name)?;
    if let Err(error) = write_and_sync(&mut file, bytes) {
        drop(file);
        let _ = unlink_at(parent, &tmp_name, 0);
        return Err(error);
    }
    drop(file);
    let published = match mode {
        ChildWrite::CreateNew => link_new_at(parent, &tmp_name, dest_name),
        ChildWrite::Replace => replace_at(parent, &tmp_name, dest_name),
    };
    match mode {
        ChildWrite::CreateNew => {
            let _ = unlink_at(parent, &tmp_name, 0);
        }
        ChildWrite::Replace => {
            if published.is_err() {
                let _ = unlink_at(parent, &tmp_name, 0);
            }
        }
    }
    published
}

fn write_and_sync(file: &mut File, bytes: &[u8]) -> io::Result<()> {
    file.write_all(bytes)?;
    crate::sync_file(file)
}

fn create_temp_at(parent: BorrowedFd<'_>, base_name: &OsStr) -> io::Result<(File, CString)> {
    let mut last_exists = None;
    for _ in 0..MAX_TEMP_ATTEMPTS {
        let tmp_os = unique_child_tmp_name(base_name);
        let tmp_name = CString::new(tmp_os.as_bytes())?;
        // SAFETY: `tmp_name` is NUL-terminated and `parent` stays open for the call.
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                tmp_name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o666,
            )
        };
        if fd >= 0 {
            // SAFETY: `fd` was just opened and nothing else owns it.
            let file = File::from(unsafe { OwnedFd::from_raw_fd(fd) });
            return Ok((file, tmp_name));
        }
        let error = io::Error::last_os_error();
        if error.kind() == ErrorKind::AlreadyExists {
            last_exists = Some(error);
        } else {
            return Err(error);
        }
    }
    Err(last_exists.unwrap_or_else(|| {
        io::Error::new(ErrorKind::AlreadyExists, "temporary child collided")
    }))
}

fn link_new_at(parent: BorrowedFd<'_>, tmp_name: &CStr, dest_name: &CStr) -> io::Result<()> {
    // SAFETY: both names are NUL-terminated and `parent` stays open; `flags == 0`
    // never follows symlinks at `dest_name` and fails with `EEXIST` if taken.
    let rc = unsafe {
        libc::linkat(
            parent.as_raw_fd(),
            tmp_name.as_ptr(),
            parent.as_raw_fd(),
            dest_name.as_ptr(),
            0,
        )
    };
    if rc == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

fn replace_at(parent: BorrowedFd<'_>, tmp_name: &CStr, dest_name: &CStr) -> io::Result<()> {
    if is_symlink_at(parent, dest_name)? {
        return Err(io::Error::new(ErrorKind::InvalidInput, "refusing to replace symlink child"));
    }
    // SAFETY: both names are NUL-terminated and `parent` stays open for the call.
    let rc = unsafe {
        libc::renameat(
            parent.as_raw_fd(),
            tmp_name.as_ptr(),
            parent.as_raw_fd(),
            dest_name.as_ptr(),
        )
    };
    if rc == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}
