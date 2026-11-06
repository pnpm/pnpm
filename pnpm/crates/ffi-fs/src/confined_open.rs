//! Opening a descendant directory of a handle without following a symlink
//! anywhere along the way.
//!
//! Each platform resolves the whole path in the kernel where it can, which
//! is one open however deep the path is. Where it cannot, the components
//! are opened one at a time from the pinned handle, which is correct but
//! costs an open per component. [`Opens`] reports which happened, because
//! the caller counts navigation opens.

use std::{
    fs::File,
    io,
    path::{Component, Path},
};

/// How many directory opens reaching a descendant took.
///
/// A caller that budgets descriptor traffic needs this, and it is not
/// inferable from the path: whether the kernel resolves the whole path at
/// once depends on the platform and, on macOS and Linux, on the running
/// kernel.
pub type Opens = usize;

/// Open the descendant `path` of `root` without following a symlink at any
/// component, including the last.
///
/// `path` must be relative and made of plain components: an absolute path,
/// a `..`, or a root or prefix component is rejected with
/// [`io::ErrorKind::InvalidInput`] rather than resolved, since confining
/// the lookup is the whole point of the call.
pub fn open_directory_beneath(root: &File, path: &Path) -> io::Result<(File, Opens)> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected a relative descendant path",
        ));
    }
    platform::open_directory_beneath(root, path)
}

/// Open each component from the handle above it, which is the fallback for
/// a platform or a kernel without whole-path confinement. `cap_primitives`
/// refuses a symlink at the component it opens, so refusing one at every
/// component is what walking them gives.
#[cfg(not(windows))]
fn open_components(root: &File, path: &Path) -> io::Result<(File, Opens)> {
    let mut components = path.components();
    let first = components.next().expect("a nonempty descendant path has a first component");
    let mut directory = cap_primitives::fs::open_dir_nofollow(root, Path::new(first.as_os_str()))?;
    let mut opens = 1;
    for component in components {
        directory =
            cap_primitives::fs::open_dir_nofollow(&directory, Path::new(component.as_os_str()))?;
        opens += 1;
    }
    Ok((directory, opens))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod platform {
    use super::Opens;
    use std::{
        ffi::CString,
        fs::File,
        io,
        os::{
            fd::{AsRawFd as _, FromRawFd as _, OwnedFd},
            unix::ffi::OsStrExt as _,
        },
        path::Path,
    };

    pub(super) fn open_directory_beneath(root: &File, path: &Path) -> io::Result<(File, Opens)> {
        #[cfg(target_os = "macos")]
        if !resolves_whole_paths() {
            return super::open_components(root, path);
        }
        let name = CString::new(path.as_os_str().as_bytes())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let descriptor = open_nofollow(root, &name);
        if descriptor >= 0 {
            // SAFETY: the open returned a descriptor that is new and owned
            // by this call, so no other owner exists to close it.
            let owned = unsafe { OwnedFd::from_raw_fd(descriptor) };
            return Ok((File::from(owned), 1));
        }
        let error = io::Error::last_os_error();
        // A kernel without the call, or without these resolve flags,
        // reports it this way rather than by ignoring them.
        if matches!(error.raw_os_error(), Some(libc::ENOSYS | libc::EINVAL)) {
            return super::open_components(root, path);
        }
        Err(error)
    }

    /// `openat2` confines the whole lookup to `root` and refuses every
    /// symlink in it, which `cap_primitives` can only do one component at a
    /// time.
    #[cfg(target_os = "linux")]
    fn open_nofollow(root: &File, name: &CString) -> libc::c_int {
        // SAFETY: `open_how` holds nothing but integers, and the kernel
        // requires its reserved words to be zero, so an all-zero value is
        // both a valid `open_how` and the only correct starting point. The
        // struct is `#[non_exhaustive]`, so a literal cannot name the
        // reserved words to zero them.
        let mut how: libc::open_how = unsafe { std::mem::zeroed() };
        how.flags = (libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) as u64;
        how.resolve = libc::RESOLVE_BENEATH | libc::RESOLVE_NO_SYMLINKS;
        // SAFETY: `name` is NUL-terminated and borrowed for the whole call,
        // the `&File` keeps the root descriptor open across it, and `how`
        // is a fully initialized stack local whose size is passed as the
        // call requires. A descriptor number fits `c_int` by definition, so
        // the narrowing cannot lose a successful result, and a failure is
        // -1 either way.
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

    /// `O_NOFOLLOW_ANY` arrived in macOS 11, which is Darwin 20. An older
    /// XNU ignores an open flag it does not know instead of failing, so the
    /// version is checked rather than inferred from an `EINVAL` that would
    /// never come.
    #[cfg(target_os = "macos")]
    fn resolves_whole_paths() -> bool {
        static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *SUPPORTED.get_or_init(|| {
            pnpm_ffi_process::darwin_major_version().is_some_and(|major| major >= 20)
        })
    }

    #[cfg(target_os = "macos")]
    fn open_nofollow(root: &File, name: &CString) -> libc::c_int {
        // SAFETY: `name` is NUL-terminated and borrowed for the whole call,
        // and the `&File` keeps the root descriptor open across it. The
        // caller rejected every component but a plain one, and
        // `O_NOFOLLOW_ANY` refuses a symlink at each of them.
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
    use super::Opens;
    use std::{
        fs::File,
        io,
        os::windows::{
            ffi::OsStrExt as _,
            io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle},
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

    pub(super) fn open_directory_beneath(root: &File, path: &Path) -> io::Result<(File, Opens)> {
        let path: std::path::PathBuf = path.components().collect();
        // Held in its own binding for the whole function: the
        // `UNICODE_STRING` below borrows it, and `NtCreateFile` reads
        // through that borrow.
        let mut name_units: Vec<u16> = path.as_os_str().encode_wide().collect();
        let length = measure(&name_units)?;
        let name = UNICODE_STRING {
            Length: length,
            MaximumLength: length,
            Buffer: name_units.as_mut_ptr(),
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: root.as_raw_handle(),
            ObjectName: &raw const name,
            // Refuses every reparse point on the path, intermediate
            // junctions included, which is this platform's whole-path
            // confinement.
            Attributes: OBJ_DONT_REPARSE,
            ..Default::default()
        };
        let mut status = IO_STATUS_BLOCK::default();
        let mut handle = ptr::null_mut();
        // SAFETY: every pointer is to a stack local that outlives this
        // synchronous call: `handle` and `status` are written, `attributes`
        // is read, and the name it points at borrows `name_units`, which is
        // alive until the function returns. The caller rejected every
        // component but a plain one.
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
        check(result)?;
        // SAFETY: the call succeeded, so it wrote a handle that is new and
        // owned by this call, and `name_units` outlived every read of it.
        let owned = unsafe { OwnedHandle::from_raw_handle(handle) };
        Ok((File::from(owned), 1))
    }

    /// A `UNICODE_STRING`'s length is in bytes, and its buffer is not
    /// NUL-terminated, so an interior NUL would be stored as part of the
    /// name rather than ending it.
    fn measure(name: &[u16]) -> io::Result<u16> {
        if name.contains(&0) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "directory path contains NUL"));
        }
        u16::try_from(size_of_val(name))
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "directory path is too long"))
    }

    fn check(status: i32) -> io::Result<()> {
        if status == STATUS_REPARSE_POINT_ENCOUNTERED {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "directory path contains a reparse point",
            ));
        }
        if status < 0 {
            // SAFETY: the translation reads the status by value and
            // dereferences nothing.
            let code = unsafe { RtlNtStatusToDosError(status) };
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod platform {
    use super::Opens;
    use std::{fs::File, io, path::Path};

    pub(super) fn open_directory_beneath(root: &File, path: &Path) -> io::Result<(File, Opens)> {
        super::open_components(root, path)
    }
}

#[cfg(all(test, not(windows)))]
mod tests;
