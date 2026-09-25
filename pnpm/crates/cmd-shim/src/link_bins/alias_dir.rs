use std::{
    ffi::{CString, OsStr, OsString},
    fs::File,
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::{OsStrExt, OsStringExt},
    },
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

static TEMPORARY_ALIAS_ID: AtomicU64 = AtomicU64::new(0);

pub(super) struct AliasDirectory {
    directory: File,
}

impl AliasDirectory {
    pub(super) fn open(path: &Path, create: bool) -> io::Result<Option<Self>> {
        let name =
            path.file_name().ok_or_else(|| invalid_input("bin alias directory has no name"))?;
        let name = c_name(name)?;
        let parent_path =
            path.parent().ok_or_else(|| invalid_input("bin alias directory has no parent"))?;
        let parent = match File::open(parent_path) {
            Ok(parent) => parent,
            Err(error) if !create && error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        match open_child(&parent, &name) {
            Ok(directory) => Ok(Some(Self { directory })),
            Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                match create_child(&parent, &name) {
                    Ok(()) => {}
                    Err(error) if error.raw_os_error() == Some(libc::EEXIST) => {}
                    Err(error) => return Err(error),
                }
                open_child(&parent, &name).map(|directory| Some(Self { directory }))
            }
            Err(error) if !create && error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(super) fn symlink_points_at(&self, name: &OsStr, target: &Path) -> io::Result<bool> {
        Ok(read_link(&self.directory, &c_name(name)?).is_ok_and(|current| current == target))
    }

    pub(super) fn replace_symlink(&self, name: &OsStr, target: &Path) -> io::Result<()> {
        let name = c_name(name)?;
        let target = c_name(target.as_os_str())?;
        let temporary = loop {
            let name = temporary_name()?;
            match symlink_at(&self.directory, &target, &name) {
                Ok(()) => break name,
                Err(error) if error.raw_os_error() == Some(libc::EEXIST) => {}
                Err(error) => return Err(error),
            }
        };
        if let Err(error) = rename_at(&self.directory, &temporary, &name) {
            let _ = unlink_at(&self.directory, &temporary, 0);
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn remove_entry(&self, name: &OsStr) -> io::Result<()> {
        match unlink_at(&self.directory, &c_name(name)?, 0) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

fn open_child(parent: &File, name: &CString) -> io::Result<File> {
    // SAFETY: `name` is NUL-terminated, `parent` owns a live descriptor, and a
    // successful descriptor is transferred to `File` before this call returns.
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )
    };
    file_from_descriptor(descriptor)
}

fn create_child(parent: &File, name: &CString) -> io::Result<()> {
    // SAFETY: `name` is NUL-terminated and `parent` owns a live directory descriptor.
    if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o777) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn temporary_name() -> io::Result<CString> {
    let id = TEMPORARY_ALIAS_ID.fetch_add(1, Ordering::Relaxed);
    c_name(OsStr::new(&format!(".pnpm-bin-{}-{id}", std::process::id())))
}

fn symlink_at(directory: &File, target: &CString, name: &CString) -> io::Result<()> {
    // SAFETY: both C strings are NUL-terminated and `directory` owns a live descriptor.
    if unsafe { libc::symlinkat(target.as_ptr(), directory.as_raw_fd(), name.as_ptr()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn rename_at(directory: &File, source: &CString, destination: &CString) -> io::Result<()> {
    // SAFETY: both C strings are NUL-terminated and `directory` owns a live descriptor.
    if unsafe {
        libc::renameat(
            directory.as_raw_fd(),
            source.as_ptr(),
            directory.as_raw_fd(),
            destination.as_ptr(),
        )
    } == 0
    {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn unlink_at(directory: &File, name: &CString, flags: libc::c_int) -> io::Result<()> {
    // SAFETY: `name` is NUL-terminated and `directory` owns a live descriptor.
    if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), flags) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn read_link(directory: &File, name: &CString) -> io::Result<std::path::PathBuf> {
    let mut capacity = 256;
    loop {
        let mut contents = Vec::<u8>::with_capacity(capacity);
        // SAFETY: `name` is NUL-terminated, `directory` owns a live descriptor,
        // and the spare capacity exposes `capacity` writable bytes.
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
            unsafe { contents.set_len(length) };
            return Ok(OsString::from_vec(contents).into());
        }
        capacity *= 2;
    }
}

fn file_from_descriptor(descriptor: libc::c_int) -> io::Result<File> {
    if descriptor == -1 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: a successful `openat` returned a new descriptor owned by this `File`.
        Ok(unsafe { File::from_raw_fd(descriptor) })
    }
}

fn c_name(name: &OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes()).map_err(|_| invalid_input("path contains a NUL byte"))
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
