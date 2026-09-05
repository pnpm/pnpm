use miette::{IntoDiagnostic, Result, WrapErr};
use std::{
    ffi::{OsStr, OsString},
    fs,
    io::{self, Read as _},
    path::{Component, Path, PathBuf},
};

#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
static TEMPORARY_FILE_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, PartialEq, Eq)]
enum FileState {
    Missing,
    Regular {
        contents: Vec<u8>,
        #[cfg(unix)]
        mode: u32,
    },
    Symlink(PathBuf),
}

pub(super) struct MetadataFile {
    path: PathBuf,
    parent: PinnedDirectory,
    remaining_parent: Vec<OsString>,
    name: OsString,
    state: FileState,
}

impl MetadataFile {
    pub(super) fn capture(path: PathBuf) -> Result<Self> {
        let path = absolute_path(path)?;
        let name = path
            .file_name()
            .ok_or_else(|| miette::miette!("metadata path has no file name: {}", path.display()))?
            .to_os_string();
        let parent_path = path
            .parent()
            .ok_or_else(|| miette::miette!("metadata path has no parent: {}", path.display()))?;
        let (parent, remaining_parent) = PinnedDirectory::nearest_existing(parent_path)
            .wrap_err_with(|| format!("pin parent of {}", path.display()))?;
        let state = read_from(&parent, &remaining_parent, &name)
            .into_diagnostic()
            .wrap_err_with(|| format!("snapshot {}", path.display()))?;
        Ok(Self { path, parent, remaining_parent, name, state })
    }

    pub(super) fn restore(self) -> Result<()> {
        let current = read_from(&self.parent, &self.remaining_parent, &self.name)
            .into_diagnostic()
            .wrap_err_with(|| format!("inspect {} before restoration", self.path.display()))?;
        if current == self.state {
            return Ok(());
        }
        let parent =
            self.parent.open_descendant(&self.remaining_parent).into_diagnostic().wrap_err_with(
                || format!("open parent of {} for restoration", self.path.display()),
            )?;
        let outcome = match self.state {
            FileState::Missing => remove_file(&parent, &self.name),
            FileState::Regular {
                contents,
                #[cfg(unix)]
                mode,
            } => write_file(
                &parent,
                &self.name,
                &contents,
                #[cfg(unix)]
                mode,
            ),
            FileState::Symlink(target) => write_symlink(&parent, &self.name, &target),
        };
        outcome.into_diagnostic().wrap_err_with(|| format!("restore {}", self.path.display()))
    }
}

fn absolute_path(path: PathBuf) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().into_diagnostic()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(miette::miette!(
                        "metadata path escapes its root: {}",
                        path.display()
                    ));
                }
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

struct PinnedDirectory {
    path: PathBuf,
    #[cfg(unix)]
    handle: fs::File,
    #[cfg(windows)]
    handles: Vec<fs::File>,
}

impl PinnedDirectory {
    fn nearest_existing(path: &Path) -> Result<(Self, Vec<OsString>)> {
        let mut existing = path;
        let mut remaining = Vec::new();
        loop {
            match fs::symlink_metadata(existing) {
                Ok(_) => break,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    let name = existing.file_name().ok_or_else(|| {
                        miette::miette!("no existing ancestor for {}", path.display())
                    })?;
                    remaining.push(name.to_os_string());
                    existing = existing.parent().ok_or_else(|| {
                        miette::miette!("no existing ancestor for {}", path.display())
                    })?;
                }
                Err(error) => {
                    return Err(error)
                        .into_diagnostic()
                        .wrap_err_with(|| format!("inspect directory {}", existing.display()));
                }
            }
        }
        remaining.reverse();
        let canonical = fs::canonicalize(existing)
            .into_diagnostic()
            .wrap_err_with(|| format!("resolve directory {}", existing.display()))?;
        let directory = Self::open(canonical)?;
        Ok((directory, remaining))
    }

    #[cfg(unix)]
    fn open(path: PathBuf) -> Result<Self> {
        use std::os::unix::fs::OpenOptionsExt as _;

        let mut options = fs::OpenOptions::new();
        options.read(true).custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY);
        let handle = options
            .open(&path)
            .into_diagnostic()
            .wrap_err_with(|| format!("open directory {}", path.display()))?;
        Ok(Self { path, handle })
    }

    #[cfg(windows)]
    fn open(path: PathBuf) -> Result<Self> {
        let handle = open_windows_directory(&path)
            .into_diagnostic()
            .wrap_err_with(|| format!("open directory {}", path.display()))?;
        ensure_real_windows_directory(&handle, &path)?;
        Ok(Self { path, handles: vec![handle] })
    }

    fn open_descendant(&self, components: &[OsString]) -> io::Result<Self> {
        #[cfg(unix)]
        let mut directory = Self { path: self.path.clone(), handle: self.handle.try_clone()? };
        #[cfg(windows)]
        let mut directory = Self {
            path: self.path.clone(),
            handles: self.handles.iter().map(fs::File::try_clone).collect::<io::Result<_>>()?,
        };
        for component in components {
            directory = directory.open_child(component)?;
        }
        Ok(directory)
    }

    #[cfg(unix)]
    fn open_child(&self, name: &OsStr) -> io::Result<Self> {
        use std::os::{fd::AsRawFd as _, unix::ffi::OsStrExt as _};

        let name = std::ffi::CString::new(name.as_bytes())?;
        // SAFETY: `name` is NUL-terminated, the parent descriptor remains
        // valid for the call, and the returned descriptor is owned on success.
        let descriptor = unsafe {
            libc::openat(
                self.handle.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            )
        };
        let handle = file_from_descriptor(descriptor)?;
        Ok(Self { path: self.path.join(OsStr::from_bytes(name.as_bytes())), handle })
    }

    #[cfg(windows)]
    fn open_child(&self, name: &OsStr) -> io::Result<Self> {
        let path = self.path.join(name);
        let handle = open_windows_directory(&path)?;
        ensure_real_windows_directory_io(&handle, &path)?;
        let mut handles =
            self.handles.iter().map(fs::File::try_clone).collect::<io::Result<Vec<_>>>()?;
        handles.push(handle);
        Ok(Self { path, handles })
    }
}

fn read_from(
    base: &PinnedDirectory,
    parent_components: &[OsString],
    name: &OsStr,
) -> io::Result<FileState> {
    let parent = match base.open_descendant(parent_components) {
        Ok(parent) => parent,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(FileState::Missing),
        Err(error) => return Err(error),
    };
    read_file(&parent, name)
}

#[cfg(unix)]
fn read_file(parent: &PinnedDirectory, name: &OsStr) -> io::Result<FileState> {
    use std::os::{
        fd::AsRawFd as _,
        unix::{ffi::OsStrExt as _, fs::PermissionsExt as _},
    };

    let name = std::ffi::CString::new(name.as_bytes())?;
    match read_link_at(&parent.handle, &name) {
        Ok(target) => return Ok(FileState::Symlink(target)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(FileState::Missing),
        Err(error) if error.raw_os_error() == Some(libc::EINVAL) => {}
        Err(error) => return Err(error),
    }
    // SAFETY: `name` is NUL-terminated and the pinned parent remains valid.
    let descriptor = unsafe {
        libc::openat(
            parent.handle.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    let mut file = match file_from_descriptor(descriptor) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(FileState::Missing),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::other("project metadata path is not a regular file or symlink"));
    }
    let mode = metadata.permissions().mode();
    let mut contents = Vec::new();
    #[expect(
        clippy::verbose_file_reads,
        reason = "the descriptor-relative open is what prevents a symlink race"
    )]
    file.read_to_end(&mut contents)?;
    Ok(FileState::Regular { contents, mode })
}

#[cfg(windows)]
fn read_file(parent: &PinnedDirectory, name: &OsStr) -> io::Result<FileState> {
    let path = parent.path.join(name);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(FileState::Missing),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() {
        return fs::read_link(path).map(FileState::Symlink);
    }
    if !metadata.is_file() || is_windows_reparse_point(&metadata) {
        return Err(io::Error::other("project metadata path is not a regular file or symlink"));
    }
    fs::read(path).map(|contents| FileState::Regular { contents })
}

#[cfg(unix)]
fn write_file(
    parent: &PinnedDirectory,
    name: &OsStr,
    contents: &[u8],
    mode: u32,
) -> io::Result<()> {
    use std::io::Write as _;
    use std::os::{unix::ffi::OsStrExt as _, unix::fs::PermissionsExt as _};

    let destination = std::ffi::CString::new(name.as_bytes())?;
    let (temporary, mut file) = create_temporary_file(parent, name)?;
    let result = (|| {
        file.write_all(contents)?;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        file.sync_all()?;
        rename_at(parent, &temporary, &destination)
    })();
    drop(file);
    if result.is_err() {
        let _ = unlink_at(parent, &temporary);
    }
    result
}

#[cfg(windows)]
fn write_file(parent: &PinnedDirectory, name: &OsStr, contents: &[u8]) -> io::Result<()> {
    pnpm_fs::write_atomic(&parent.path.join(name), contents)
}

#[cfg(unix)]
fn write_symlink(parent: &PinnedDirectory, name: &OsStr, target: &Path) -> io::Result<()> {
    use std::os::{fd::AsRawFd as _, unix::ffi::OsStrExt as _};

    let destination = std::ffi::CString::new(name.as_bytes())?;
    let target = std::ffi::CString::new(target.as_os_str().as_bytes())?;
    let temporary = temporary_name(name)?;
    // SAFETY: the target and temporary name are NUL-terminated, and the
    // directory descriptor remains valid.
    if unsafe { libc::symlinkat(target.as_ptr(), parent.handle.as_raw_fd(), temporary.as_ptr()) }
        != 0
    {
        return Err(io::Error::last_os_error());
    }
    let result = rename_at(parent, &temporary, &destination);
    if result.is_err() {
        let _ = unlink_at(parent, &temporary);
    }
    result
}

#[cfg(windows)]
fn write_symlink(parent: &PinnedDirectory, name: &OsStr, target: &Path) -> io::Result<()> {
    let path = parent.path.join(name);
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::os::windows::fs::symlink_file(target, path)
}

#[cfg(unix)]
fn remove_file(parent: &PinnedDirectory, name: &OsStr) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt as _;

    let name = std::ffi::CString::new(name.as_bytes())?;
    match unlink_at(parent, &name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn remove_file(parent: &PinnedDirectory, name: &OsStr) -> io::Result<()> {
    match fs::remove_file(parent.path.join(name)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn create_temporary_file(
    parent: &PinnedDirectory,
    name: &OsStr,
) -> io::Result<(std::ffi::CString, fs::File)> {
    use std::os::fd::AsRawFd as _;

    loop {
        let temporary = temporary_name(name)?;
        // SAFETY: the name is NUL-terminated, the directory descriptor remains
        // valid, and a successful descriptor is immediately owned.
        let descriptor = unsafe {
            libc::openat(
                parent.handle.as_raw_fd(),
                temporary.as_ptr(),
                libc::O_WRONLY | libc::O_CLOEXEC | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
                0o600,
            )
        };
        match file_from_descriptor(descriptor) {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

#[cfg(unix)]
fn temporary_name(name: &OsStr) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt as _;

    let mut temporary = OsString::from(".");
    temporary.push(name);
    temporary.push(format!(
        ".pnpm-{}-{}",
        std::process::id(),
        TEMPORARY_FILE_ID.fetch_add(1, Ordering::Relaxed),
    ));
    Ok(std::ffi::CString::new(temporary.as_bytes())?)
}

#[cfg(unix)]
fn rename_at(
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

#[cfg(unix)]
fn unlink_at(parent: &PinnedDirectory, name: &std::ffi::CStr) -> io::Result<()> {
    use std::os::fd::AsRawFd as _;

    // SAFETY: the name and pinned directory descriptor remain valid.
    if unsafe { libc::unlinkat(parent.handle.as_raw_fd(), name.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn read_link_at(directory: &fs::File, name: &std::ffi::CStr) -> io::Result<PathBuf> {
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

#[cfg(unix)]
fn file_from_descriptor(descriptor: libc::c_int) -> io::Result<fs::File> {
    use std::os::fd::{FromRawFd as _, OwnedFd};

    if descriptor == -1 {
        Err(io::Error::last_os_error())
    } else {
        // SAFETY: a successful `openat` returned a new owned descriptor.
        let descriptor = unsafe { OwnedFd::from_raw_fd(descriptor) };
        Ok(fs::File::from(descriptor))
    }
}

#[cfg(windows)]
fn open_windows_directory(path: &Path) -> io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(windows)]
fn ensure_real_windows_directory(handle: &fs::File, path: &Path) -> Result<()> {
    ensure_real_windows_directory_io(handle, path)
        .into_diagnostic()
        .wrap_err_with(|| format!("inspect directory {}", path.display()))
}

#[cfg(windows)]
fn ensure_real_windows_directory_io(handle: &fs::File, path: &Path) -> io::Result<()> {
    let metadata = handle.metadata()?;
    if metadata.is_dir() && !is_windows_reparse_point(&metadata) {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "managed metadata directory {} must be a real directory",
            path.display(),
        )))
    }
}

#[cfg(windows)]
fn is_windows_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}
