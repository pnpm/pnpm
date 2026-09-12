#[cfg(unix)]
use super::file_from_descriptor;
use super::{IntoDiagnostic, OsStr, OsString, Path, PathBuf, Result, fs, io};
use miette::WrapErr as _;

pub(super) struct PinnedDirectory {
    pub(super) path: PathBuf,
    #[cfg(unix)]
    pub(super) handle: fs::File,
    #[cfg(windows)]
    pub(super) handles: Vec<fs::File>,
}

impl PinnedDirectory {
    pub(super) fn nearest_existing(path: &Path) -> Result<(Self, Vec<OsString>)> {
        let path_display = path.display().to_string();
        let mut existing = path;
        let mut remaining = Vec::new();
        loop {
            match fs::symlink_metadata(existing) {
                Ok(_) => break,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    let name = existing.file_name().ok_or_else(|| {
                        miette::miette!("no existing ancestor for {path_display}")
                    })?;
                    remaining.push(name.to_os_string());
                    existing = existing.parent().ok_or_else(|| {
                        miette::miette!("no existing ancestor for {path_display}")
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

    pub(super) fn open_descendant(&self, components: &[OsString]) -> io::Result<Self> {
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

#[cfg(windows)]
fn open_windows_directory(path: &Path) -> io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    // Deliberately omit FILE_SHARE_DELETE. Every component handle remains
    // alive in `PinnedDirectory`, so Windows cannot rename or replace a
    // validated parent before the later pathname-based child operation.
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
pub(super) fn is_windows_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}
