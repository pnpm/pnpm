use std::{ffi::CString, io, path::Path};

pub(crate) fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    let target = CString::new(target.as_os_str().as_encoded_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let link = CString::new(link.as_os_str().as_encoded_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    // SAFETY: both pointers refer to live, NUL-terminated strings. symlink
    // only reads them and retains neither pointer after returning.
    if unsafe { libc::symlink(target.as_ptr(), link.as_ptr()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[derive(PartialEq, Eq)]
pub(crate) struct FileIdentity {
    device: libc::dev_t,
    inode: libc::ino_t,
}

pub(crate) fn path_identity(path: &Path) -> io::Result<FileIdentity> {
    let path = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: path is NUL-terminated and metadata points to writable storage
    // of the exact type lstat initializes on success.
    if unsafe { libc::lstat(path.as_ptr(), metadata.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful lstat initialized metadata.
    let metadata = unsafe { metadata.assume_init() };
    Ok(FileIdentity { device: metadata.st_dev, inode: metadata.st_ino })
}

pub(crate) fn file_identity(file: &std::fs::File) -> io::Result<FileIdentity> {
    let metadata = file_metadata(file)?;
    Ok(FileIdentity { device: metadata.st_dev, inode: metadata.st_ino })
}

/// Count hard links to an already opened file.
pub fn file_link_count(file: &std::fs::File) -> io::Result<u64> {
    file_metadata(file).map(|metadata| metadata.st_nlink)
}

fn file_metadata(file: &std::fs::File) -> io::Result<libc::stat> {
    use std::os::fd::AsRawFd;
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: file keeps its descriptor open and metadata is writable
    // storage of the exact type fstat initializes on success.
    if unsafe { libc::fstat(file.as_raw_fd(), metadata.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful fstat initialized metadata.
    Ok(unsafe { metadata.assume_init() })
}

pub(crate) fn open_for_overwrite(path: &Path) -> io::Result<std::fs::File> {
    use std::os::fd::FromRawFd;
    let path = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    // SAFETY: path is NUL-terminated; these flags do not create a file,
    // so open takes no variadic mode argument.
    let descriptor =
        unsafe { libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful open returned a descriptor owned by this call.
    Ok(unsafe { std::fs::File::from_raw_fd(descriptor) })
}

/// Exclusively create a file with the requested host permission bits.
pub fn create_new(path: &Path, mode: u32) -> io::Result<std::fs::File> {
    with_absolute_path(path, |path| {
        let mut descriptor = 0;
        // SAFETY: the host reads the live path slice and initializes the output
        // synchronously. No pointer is retained across the imported call.
        check(unsafe { host::create_new(path.as_ptr(), path.len(), mode, &raw mut descriptor) })?;
        owned_file(descriptor)
    })
}

/// Open a host file without following its final symlink or blocking on a FIFO.
pub fn open_nofollow(path: &Path) -> io::Result<std::fs::File> {
    with_absolute_path(path, |path| {
        let mut descriptor = 0;
        // SAFETY: the host reads the live path slice and initializes the output
        // synchronously. No pointer is retained across the imported call.
        check(unsafe { host::open_nofollow(path.as_ptr(), path.len(), &raw mut descriptor) })?;
        owned_file(descriptor)
    })
}

/// Open a descendant directory after rejecting symlinks below `template`.
/// The template itself may be a symlink. The host's path walk and final open
/// are separate operations because the WebContainer host does not provide
/// descriptor-stable `openat`.
pub fn open_directory_nofollow_beneath(path: &Path, template: &Path) -> io::Result<std::fs::File> {
    with_absolute_path(path, |path| {
        with_absolute_path(template, |template| {
            let mut descriptor = 0;
            // SAFETY: both live path slices and the writable output remain
            // valid for the synchronous imported call.
            check(unsafe {
                host::open_directory_nofollow_beneath(
                    path.as_ptr(),
                    path.len(),
                    template.as_ptr(),
                    template.len(),
                    &raw mut descriptor,
                )
            })?;
            owned_file(descriptor)
        })
    })
}

/// Grant inherited bits through Linux directory handles when the host can
/// resolve `/proc/self/fd`. Other hosts return `Unsupported` for the caller's
/// WASI descriptor fallback.
pub fn grant_directory_mode_beneath(path: &Path, template: &Path, extra: u32) -> io::Result<()> {
    with_absolute_path(path, |path| {
        with_absolute_path(template, |template| {
            // SAFETY: both path slices stay live until the synchronous host
            // call returns; it retains neither pointer.
            check(unsafe {
                host::grant_directory_mode_beneath(
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

fn owned_file(descriptor: u32) -> io::Result<std::fs::File> {
    use std::os::fd::FromRawFd;
    let descriptor = descriptor.try_into().map_err(|_| io::ErrorKind::InvalidData)?;
    // SAFETY: successful host open transfers ownership of this WASI descriptor.
    Ok(unsafe { std::fs::File::from_raw_fd(descriptor) })
}

pub(crate) fn file_mode(file: &std::fs::File) -> io::Result<u32> {
    use std::os::fd::AsRawFd;
    let mut mode = 0;
    // SAFETY: file keeps the descriptor alive and mode is writable for the
    // synchronous imported call.
    check(unsafe { host::fmode(file.as_raw_fd(), &raw mut mode) })?;
    Ok(mode)
}

/// Checks execute access using the supervising process's credentials.
pub fn executable_access(path: &Path) -> io::Result<()> {
    with_absolute_path(path, |path| {
        // SAFETY: the host reads the live path slice synchronously and retains no pointer.
        check(unsafe { host::path_access_executable(path.as_ptr(), path.len()) })
    })
}

/// Apply database permissions atomically when its `SQLite` sidecars are created.
pub fn register_sqlite_permissions(path: &Path, registered: bool) -> io::Result<()> {
    with_absolute_path(path, |path| {
        // SAFETY: the host reads the live path slice synchronously and retains no pointer.
        check(unsafe { host::sqlite_register(path.as_ptr(), path.len(), u32::from(registered)) })
    })
}

/// Refuse cross-user stores whose private lease namespaces cannot coordinate.
pub fn check_file_owner(file: &std::fs::File) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: file keeps its WASI descriptor live for the synchronous host call.
    check(unsafe { host::check_owner(file.as_raw_fd()) })
}

pub(crate) fn set_file_mode(file: &std::fs::File, mode: u32) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: file keeps its WASI descriptor alive for the imported call.
    check(unsafe { host::fchmod(file.as_raw_fd(), mode) })
}

pub(crate) fn path_mode(path: &Path) -> io::Result<u32> {
    with_absolute_path(path, |path| {
        let mut mode = 0;
        // SAFETY: path is readable and mode is writable until the synchronous
        // call completes; the host retains neither pointer.
        check(unsafe { host::lmode(path.as_ptr(), path.len(), &raw mut mode) })?;
        Ok(mode)
    })
}

pub(crate) fn user_id() -> u32 {
    // SAFETY: this import reads the host user identity and takes no pointers.
    unsafe { host::uid() }
}

pub(crate) fn secure_directory(path: &Path) -> io::Result<()> {
    with_absolute_path(path, |path| {
        // SAFETY: the host reads the live path slice synchronously.
        check(unsafe { host::secure_directory(path.as_ptr(), path.len()) })
    })
}

pub(crate) fn current_umask() -> u32 {
    // SAFETY: this import reads the host's mask and takes no pointers.
    unsafe { host::umask() }
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

mod host {
    #[link(wasm_import_module = "pnpm_fs")]
    unsafe extern "C" {
        pub(super) fn create_new(
            path: *const u8,
            length: usize,
            mode: u32,
            output: *mut u32,
        ) -> i32;
        pub(super) fn open_nofollow(path: *const u8, length: usize, output: *mut u32) -> i32;
        pub(super) fn open_directory_nofollow_beneath(
            path: *const u8,
            length: usize,
            template: *const u8,
            template_length: usize,
            output: *mut u32,
        ) -> i32;
        pub(super) fn grant_directory_mode_beneath(
            path: *const u8,
            length: usize,
            template: *const u8,
            template_length: usize,
            extra: u32,
        ) -> i32;
        pub(super) fn open_lock(path: *const u8, length: usize, output: *mut u32) -> i32;
        pub(super) fn try_lock(descriptor: i32, exclusive: u32) -> i32;
        pub(super) fn sqlite_register(path: *const u8, length: usize, registered: u32) -> i32;
        pub(super) fn check_owner(descriptor: i32) -> i32;
        pub(super) fn path_access_executable(path: *const u8, length: usize) -> i32;
        pub(super) fn fchmod(descriptor: i32, mode: u32) -> i32;
        pub(super) fn fmode(descriptor: i32, output: *mut u32) -> i32;
        pub(super) fn lmode(path: *const u8, length: usize, output: *mut u32) -> i32;
        pub(super) fn umask() -> u32;
        pub(super) fn uid() -> u32;
        pub(super) fn secure_directory(path: *const u8, length: usize) -> i32;
    }
}

/// Exclusively create a file with private or inherited parent permissions.
pub fn create_inheriting_mode(
    parent: &Path,
    path: &Path,
    requested: Option<u32>,
) -> io::Result<std::fs::File> {
    if requested.is_some_and(|mode| mode.trailing_zeros() >= 6) {
        return create_new(path, requested.unwrap_or(0o600));
    }
    let parent_mode = match path_mode(parent) {
        Ok(mode) if mode & libc::S_IFMT == libc::S_IFDIR => mode,
        _ => return create_new(path, requested.unwrap_or(0o666)),
    };
    let mode = crate::file_mode::inherited_file_mode(
        parent_mode,
        requested.is_some_and(crate::file_mode::is_executable),
    );
    let file = create_new(path, mode)?;
    grant_file_mode(&file, mode)?;
    Ok(file)
}

fn grant_file_mode(file: &std::fs::File, wanted: u32) -> io::Result<()> {
    let current = file_mode(file)? & 0o777;
    if current | wanted == current {
        return Ok(());
    }
    match set_file_mode(file, current | wanted) {
        Err(error)
            if matches!(error.raw_os_error(), Some(libc::EPERM | libc::EACCES | libc::EROFS)) =>
        {
            Ok(())
        }
        result => result,
    }
}

pub(crate) fn open_lock(path: &Path) -> io::Result<std::fs::File> {
    with_absolute_path(path, |path| {
        let mut descriptor = 0;
        // SAFETY: path is readable and the output descriptor is writable
        // throughout the synchronous host call.
        check(unsafe { host::open_lock(path.as_ptr(), path.len(), &raw mut descriptor) })?;
        owned_file(descriptor)
    })
}

/// Try to hold a host process lease until this file is closed.
pub fn try_lock_file(file: &std::fs::File, exclusive: bool) -> Result<(), std::fs::TryLockError> {
    use std::os::fd::AsRawFd;
    // SAFETY: the file keeps its descriptor alive throughout the host call.
    match unsafe { host::try_lock(file.as_raw_fd(), u32::from(exclusive)) } {
        0 => Ok(()),
        libc::EAGAIN => Err(std::fs::TryLockError::WouldBlock),
        errno => Err(std::fs::TryLockError::Error(io::Error::from_raw_os_error(errno))),
    }
}

/// Acquire a host file lease, failing after 30 seconds of contention.
pub fn lock_file(file: &std::fs::File, exclusive: bool) -> io::Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        match try_lock_file(file, exclusive) {
            Ok(()) => return Ok(()),
            Err(std::fs::TryLockError::WouldBlock) => {
                if std::time::Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "WebContainer file lock remained busy for 30 seconds; a nested pnpm command may be waiting on its parent",
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
        }
    }
}
