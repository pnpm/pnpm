use std::{
    fs::File,
    io,
    path::{Component, Path},
};

/// Open a descendant without following symlinks in any component. Only normal
/// relative components are accepted; no parent navigation or identity lookup is used.
pub(super) fn open_directory(root: &File, path: &Path, opens: &mut usize) -> io::Result<File> {
    if path.as_os_str().is_empty()
        || path.components().any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected a relative descendant path",
        ));
    }
    platform::open_directory(root, path, opens)
}

// Older kernels and other Unix platforms lack whole-path no-symlink resolution.
// Open each component from a pinned handle there, at the cost of depth-dependent opens.
#[cfg(not(windows))]
fn open_components(root: &File, path: &Path, opens: &mut usize) -> io::Result<File> {
    let mut components = path.components();
    *opens += 1;
    let mut directory = cap_primitives::fs::open_dir_nofollow(
        root,
        Path::new(components.next().expect("nonempty descendant path").as_os_str()),
    )?;
    for component in components {
        *opens += 1;
        directory =
            cap_primitives::fs::open_dir_nofollow(&directory, Path::new(component.as_os_str()))?;
    }
    Ok(directory)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod platform {
    use std::{
        ffi::CString,
        fs::File,
        io,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
        path::Path,
    };

    pub(super) fn open_directory(root: &File, path: &Path, opens: &mut usize) -> io::Result<File> {
        #[cfg(target_os = "macos")]
        if !supports_nofollow_any() {
            return super::open_components(root, path, opens);
        }
        let name = CString::new(path.as_os_str().as_bytes())?;
        *opens += 1;
        let fd = open_nofollow(root, &name);
        if fd >= 0 {
            // SAFETY: a successful open returns a new, uniquely owned descriptor.
            return Ok(unsafe { File::from_raw_fd(fd) });
        }
        let error = io::Error::last_os_error();
        if matches!(error.raw_os_error(), Some(libc::ENOSYS | libc::EINVAL)) {
            return super::open_components(root, path, opens);
        }
        Err(error)
    }

    #[cfg(target_os = "linux")]
    fn open_nofollow(root: &File, name: &CString) -> libc::c_int {
        // SAFETY: open_how contains only integers; zero initializes reserved fields.
        let mut how: libc::open_how = unsafe { std::mem::zeroed() };
        how.flags = (libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) as u64;
        how.resolve = libc::RESOLVE_BENEATH | libc::RESOLVE_NO_SYMLINKS;
        // SAFETY: root stays open, name is NUL-terminated, and how is initialized
        // for the supplied size. The kernel enforces confinement and no symlinks
        // across the entire lookup; cap-primitives only denies the final symlink.
        unsafe {
            libc::syscall(
                libc::SYS_openat2,
                root.as_raw_fd(),
                name.as_ptr(),
                &raw const how,
                std::mem::size_of::<libc::open_how>(),
            ) as libc::c_int
        }
    }

    #[cfg(target_os = "macos")]
    fn supports_nofollow_any() -> bool {
        static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *SUPPORTED.get_or_init(|| {
            // macOS 11 (Darwin 20) introduced O_NOFOLLOW_ANY. Older kernels can
            // ignore unknown open flags, so an EINVAL fallback alone is unsafe.
            // SAFETY: utsname contains only character arrays.
            let mut info: libc::utsname = unsafe { std::mem::zeroed() };
            // SAFETY: info is a valid writable buffer of the required size.
            if unsafe { libc::uname(&raw mut info) } != 0 {
                return false;
            }
            // SAFETY: uname succeeded and initialized the release C string.
            unsafe { std::ffi::CStr::from_ptr(info.release.as_ptr()) }
                .to_str()
                .ok()
                .and_then(|release| release.split('.').next())
                .and_then(|major| major.parse::<u32>().ok())
                .is_some_and(|major| major >= 20)
        })
    }

    #[cfg(target_os = "macos")]
    fn open_nofollow(root: &File, name: &CString) -> libc::c_int {
        // SAFETY: root stays open and name is NUL-terminated. The caller rejects
        // absolute/parent components, and O_NOFOLLOW_ANY rejects every symlink.
        unsafe {
            libc::openat(
                root.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW_ANY,
            )
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::{
        fs::File,
        io,
        os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle},
        },
        path::Path,
        ptr,
    };
    use windows_sys::{
        Wdk::{
            Foundation::OBJECT_ATTRIBUTES,
            Storage::FileSystem::{
                FILE_DIRECTORY_FILE, FILE_OPEN, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
            },
        },
        Win32::{
            Foundation::{
                OBJ_DONT_REPARSE, RtlNtStatusToDosError, STATUS_REPARSE_POINT_ENCOUNTERED,
                UNICODE_STRING,
            },
            Storage::FileSystem::{
                FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
                FILE_SHARE_WRITE, SYNCHRONIZE,
            },
            System::IO::IO_STATUS_BLOCK,
        },
    };

    pub(super) fn open_directory(root: &File, path: &Path, opens: &mut usize) -> io::Result<File> {
        let path: std::path::PathBuf = path.components().collect();
        let mut name: Vec<u16> = path.as_os_str().encode_wide().collect();
        let length = u16::try_from(name.len() * size_of::<u16>()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "directory path is too long")
        })?;
        if name.contains(&0) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "directory path contains NUL"));
        }
        let name =
            UNICODE_STRING { Length: length, MaximumLength: length, Buffer: name.as_mut_ptr() };
        let attributes = OBJECT_ATTRIBUTES {
            Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: root.as_raw_handle(),
            ObjectName: &raw const name,
            Attributes: OBJ_DONT_REPARSE,
            ..Default::default()
        };
        let mut status = IO_STATUS_BLOCK::default();
        let mut handle = ptr::null_mut();
        *opens += 1;
        // SAFETY: all buffers and the root handle outlive this synchronous call.
        // Only normal relative components reach it. OBJ_DONT_REPARSE rejects all
        // reparse points, including intermediate junctions; no identity is inferred.
        let result = unsafe {
            NtCreateFile(
                &raw mut handle,
                FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                &raw const attributes,
                &raw mut status,
                ptr::null(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                FILE_OPEN,
                FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT,
                ptr::null(),
                0,
            )
        };
        if result == STATUS_REPARSE_POINT_ENCOUNTERED {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "directory path contains a reparse point",
            ));
        }
        if result < 0 {
            // SAFETY: this function translates a status value without dereferencing memory.
            return Err(io::Error::from_raw_os_error(
                unsafe { RtlNtStatusToDosError(result) } as i32
            ));
        }
        // SAFETY: NtCreateFile succeeded and transferred a new owned handle to us.
        Ok(unsafe { File::from_raw_handle(handle) })
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod platform {
    use std::{fs::File, io, path::Path};

    pub(super) fn open_directory(root: &File, path: &Path, opens: &mut usize) -> io::Result<File> {
        super::open_components(root, path, opens)
    }
}

#[cfg(all(test, unix))]
mod tests;
