use super::{FileState, PinnedDirectory};
use std::{
    ffi::OsString,
    fs,
    io::{self, Read as _},
    os::unix::fs::PermissionsExt as _,
    path::PathBuf,
};

pub(super) fn rename_at(
    parent: &PinnedDirectory,
    source: &std::ffi::CStr,
    destination: &std::ffi::CStr,
) -> io::Result<()> {
    use std::os::fd::AsRawFd as _;

    // SAFETY: both names and the pinned directory descriptor remain valid.
    if unsafe {
        libc::renameat(
            parent.handle.as_raw_fd(),
            source.as_ptr(),
            parent.handle.as_raw_fd(),
            destination.as_ptr(),
        )
    } == 0
    {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(super) fn unlink_at(parent: &PinnedDirectory, name: &std::ffi::CStr) -> io::Result<()> {
    use std::os::fd::AsRawFd as _;

    // SAFETY: the name and pinned directory descriptor remain valid.
    if unsafe { libc::unlinkat(parent.handle.as_raw_fd(), name.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(super) fn read_link_at(directory: &fs::File, name: &std::ffi::CStr) -> io::Result<PathBuf> {
    use std::os::{fd::AsRawFd as _, unix::ffi::OsStringExt as _};

    let mut capacity = 256;
    loop {
        let mut contents = Vec::<u8>::with_capacity(capacity);
        // SAFETY: the name and directory descriptor are valid, and the buffer
        // exposes `capacity` writable bytes to `readlinkat`.
        let length = unsafe {
            libc::readlinkat(
                directory.as_raw_fd(),
                name.as_ptr(),
                contents.as_mut_ptr().cast(),
                contents.capacity(),
            )
        };
        if length == -1 {
            return Err(io::Error::last_os_error());
        }
        let length = usize::try_from(length).expect("readlinkat returned a nonnegative length");
        if length < contents.capacity() {
            // SAFETY: `readlinkat` initialized exactly `length` bytes.
            unsafe {
                contents.set_len(length);
            }
            return Ok(OsString::from_vec(contents).into());
        }
        capacity *= 2;
    }
}

pub(super) fn file_from_descriptor(descriptor: libc::c_int) -> io::Result<fs::File> {
    use std::os::fd::{FromRawFd as _, OwnedFd};

    if descriptor == -1 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: a successful `openat` returned a new owned descriptor.
        let descriptor = unsafe { OwnedFd::from_raw_fd(descriptor) };
        Ok(fs::File::from(descriptor))
    }
}

pub(super) fn read_regular_file(mut file: fs::File) -> io::Result<FileState> {
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::other(
            "project metadata path is not a regular file or symlink",
        ));
    }
    let mode = metadata.permissions().mode();
    let mut contents = Vec::new();
    #[expect(
        clippy::verbose_file_reads,
        reason = "the descriptor-relative open is what prevents a symlink race"
    )]
    file.read_to_end(&mut contents)?;
    Ok(FileState::Regular {
        contents,
        mode,
    })
}
