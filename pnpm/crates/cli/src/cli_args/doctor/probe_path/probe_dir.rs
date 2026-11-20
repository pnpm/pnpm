use std::{
    ffi::{
        OsStr,
        OsString,
    },
    fs,
    io::{
        self,
        Write as _,
    },
    path::{
        Path,
        PathBuf,
    },
};

pub(in crate::cli_args::doctor) fn random_probe_entry_name(prefix: &str) -> OsString {
    let mut bytes = [0u8; 16];
    let suffix = match getrandom::fill(&mut bytes) {
        Ok(()) => u128::from_ne_bytes(bytes),
        Err(_) => 0,
    };
    OsString::from(format!(".pnpm-{prefix}-{}-{suffix:032x}", pnpm_fs::process_id()))
}

pub(in crate::cli_args::doctor) struct ProbeDir {
    parent: pnpm_fs::OpenDir,
    dir: Option<pnpm_fs::OpenDir>,
    #[cfg(unix)]
    raw_dir: Option<fs::File>,
    #[cfg(windows)]
    pin_file: Option<(fs::File, OsString)>,
    #[cfg(test)]
    invalidated: bool,
    name: OsString,
    entries: Vec<OsString>,
}

impl ProbeDir {
    pub(in crate::cli_args::doctor) fn new(safe_ancestor: &Path) -> io::Result<Self> {
        let parent = pnpm_fs::OpenDir::open(safe_ancestor)?;
        let temp = tempfile::Builder::new().prefix(".pnpm-doctor-").tempdir_in(parent.path())?;
        let Some(name) = temp
            .path()
            .file_name()
            .map(OsStr::to_os_string)
        else {
            return Err(io::Error::other("probe directory missing file name"));
        };
        if parent.canonical_path()?.as_deref() != Some(safe_ancestor) {
            return Err(io::Error::other("probe ancestor path changed during open"));
        }
        let Some(dir) = parent.open_child_dir(&name)? else {
            return Err(io::Error::other("probe path is not a real directory"));
        };
        let expected_canonical = safe_ancestor.join(&name);
        if dir.canonical_path()?.as_deref() != Some(expected_canonical.as_path()) {
            return Err(io::Error::other("probe directory canonical path mismatch"));
        }
        #[cfg(unix)]
        let raw_dir = Some(open_verified_unix_child_dir(safe_ancestor, &name)?);
        #[cfg(windows)]
        let pin_file = Some(open_windows_pin_file(&dir, &expected_canonical)?);
        let _ = temp.keep();
        Ok(Self {
            parent,
            dir: Some(dir),
            #[cfg(unix)]
            raw_dir,
            #[cfg(windows)]
            pin_file,
            #[cfg(test)]
            invalidated: false,
            name,
            entries: Vec::with_capacity(4),
        })
    }

    pub(in crate::cli_args::doctor) fn write_new_file(
        &mut self,
        prefix: &str,
        content: &[u8],
    ) -> io::Result<PathBuf> {
        if self.is_invalidated() {
            return Err(io::Error::other("probe directory invalidated"));
        }
        let dir =
            self.dir.as_ref().ok_or_else(|| io::Error::other("probe directory unavailable"))?;
        let entry = random_probe_entry_name(prefix);
        #[cfg(unix)]
        {
            let raw_dir = self.raw_dir
                .as_ref()
                .ok_or_else(|| io::Error::other("probe directory descriptor unavailable"))?;
            create_exclusive_file_unix(raw_dir, &entry, content)?;
        }
        #[cfg(not(unix))]
        create_exclusive_file_pinned(dir, &entry, content)?;
        let entry_path = dir.path().join(&entry);
        self.entries.push(entry);
        Ok(entry_path)
    }

    pub(in crate::cli_args::doctor) fn register_entry(
        &mut self,
        prefix: &str,
    ) -> io::Result<PathBuf> {
        let dir =
            self.dir.as_ref().ok_or_else(|| io::Error::other("probe directory unavailable"))?;
        let entry = random_probe_entry_name(prefix);
        let entry_path = dir.path().join(&entry);
        self.entries.push(entry);
        Ok(entry_path)
    }

    #[cfg(test)]
    pub(in crate::cli_args::doctor) fn invalidate_for_test(&mut self) {
        self.invalidated = true;
    }

    fn is_invalidated(&self) -> bool {
        #[cfg(test)]
        {
            self.invalidated
        }
        #[cfg(not(test))]
        {
            let _ = self;
            false
        }
    }
}

impl Drop for ProbeDir {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            if let Some(raw_dir) = self.raw_dir.take() {
                for entry in &self.entries {
                    unlink_probe_entry_unix(&raw_dir, entry);
                }
            }
            self.dir = None;
        }
        #[cfg(not(unix))]
        if let Some(dir) = self.dir.take() {
            for entry in &self.entries {
                remove_pinned_probe_entry(&dir, entry);
            }
            #[cfg(windows)]
            if let Some((pin_handle, pin_name)) = self.pin_file.take() {
                drop(pin_handle);
                let _ = dir.remove_child_file(&pin_name);
            }
            drop(dir);
        }
        let _ = self.parent.remove_empty_child_dir(&self.name);
    }
}

#[cfg(unix)]
pub(super) fn create_exclusive_file_unix(
    raw_dir: &fs::File,
    name: &OsStr,
    content: &[u8],
) -> io::Result<()> {
    use std::os::{
        fd::{
            AsRawFd as _,
            FromRawFd as _,
            OwnedFd,
        },
        unix::ffi::OsStrExt as _,
    };

    let c_name = std::ffi::CString::new(name.as_bytes())?;
    let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: `c_name` is NUL-terminated and `raw_dir` stays open for the call.
    let descriptor = unsafe { libc::openat(raw_dir.as_raw_fd(), c_name.as_ptr(), flags, 0o600) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `openat` returned a fresh owned file descriptor.
    let mut file = fs::File::from(unsafe { OwnedFd::from_raw_fd(descriptor) });
    if !content.is_empty()
        && let Err(error) = file.write_all(content)
    {
        drop(file);
        unlink_probe_entry_unix(raw_dir, name);
        return Err(error);
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn create_exclusive_file_pinned(
    dir: &pnpm_fs::OpenDir,
    name: &OsStr,
    content: &[u8],
) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.path().join(name))?;
    if !content.is_empty()
        && let Err(error) = file.write_all(content)
    {
        drop(file);
        let _ = dir.remove_child_file(name);
        return Err(error);
    }
    Ok(())
}

#[cfg(windows)]
fn open_windows_pin_file(
    dir: &pnpm_fs::OpenDir,
    expected_canonical: &Path,
) -> io::Result<(fs::File, OsString)> {
    let pin_name = random_probe_entry_name("pin");
    let pin_handle = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.path().join(&pin_name))?;
    if dir.canonical_path()?.as_deref() != Some(expected_canonical) {
        drop(pin_handle);
        let _ = dir.remove_child_file(&pin_name);
        return Err(io::Error::other("probe directory changed during pin"));
    }
    Ok((pin_handle, pin_name))
}

#[cfg(unix)]
fn open_verified_unix_child_dir(safe_ancestor: &Path, child_name: &OsStr) -> io::Result<fs::File> {
    use std::os::{
        fd::{
            AsRawFd as _,
            FromRawFd as _,
            OwnedFd,
        },
        unix::{
            ffi::OsStrExt as _,
            fs::{
                MetadataExt as _,
                OpenOptionsExt as _,
            },
        },
    };

    let parent = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(safe_ancestor)?;
    let c_name = std::ffi::CString::new(child_name.as_bytes())?;
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: `c_name` is NUL-terminated and `parent` stays open for the call.
    let descriptor = unsafe { libc::openat(parent.as_raw_fd(), c_name.as_ptr(), flags) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `openat` returned a fresh owned file descriptor.
    let child = fs::File::from(unsafe { OwnedFd::from_raw_fd(descriptor) });
    let meta = child.metadata()?;
    // SAFETY: `geteuid` has no preconditions.
    if !meta.file_type().is_dir() || meta.uid() != unsafe { libc::geteuid() } {
        return Err(io::Error::other("probe directory ownership mismatch"));
    }
    Ok(child)
}

#[cfg(unix)]
pub(super) fn unlink_probe_entry_unix(raw_dir: &fs::File, entry: &OsStr) {
    use std::os::{
        fd::AsRawFd as _,
        unix::ffi::OsStrExt as _,
    };

    let Ok(c_name) = std::ffi::CString::new(entry.as_bytes()) else {
        return;
    };
    // SAFETY: `c_name` is NUL-terminated and `raw_dir` stays open for the call.
    unsafe {
        libc::unlinkat(raw_dir.as_raw_fd(), c_name.as_ptr(), 0);
    }
}

#[cfg(not(unix))]
fn remove_pinned_probe_entry(dir: &pnpm_fs::OpenDir, entry: &OsStr) {
    if matches!(dir.remove_child_file(entry), Ok(true)) {
        return;
    }
    if matches!(dir.has_symlink_child(entry), Ok(true)) {
        let _ = pnpm_fs::remove_dirent(&dir.path().join(entry));
    }
}
