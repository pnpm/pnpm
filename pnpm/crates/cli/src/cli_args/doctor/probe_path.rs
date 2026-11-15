#[cfg(any(windows, test))]
pub(super) mod windows_acl;

use super::CheckResult;
use std::{
    ffi::{OsStr, OsString},
    fs, io,
    path::{Path, PathBuf},
};

pub(super) fn check_writable_dir(title: &str, dir: &Path) -> CheckResult {
    if !can_write_to_dir(dir) {
        return CheckResult::fail(
            title,
            format!("no write access to {}", dir.display()),
            "Fix the directory permissions or point the setting at a writable path.",
        );
    }
    CheckResult::pass(title, dir.display().to_string())
}

pub(super) fn dir_is_in_path(dir: &Path, path_dirs: &[PathBuf]) -> bool {
    let canonical = dir.canonicalize();
    path_dirs
        .iter()
        .any(|entry| {
            entry == dir
                || match (&canonical, entry.canonicalize()) {
                    (Ok(dir), Ok(entry)) => dir == &entry,
                    _ => false,
                }
        })
}

/// Probe write access for `dir`: existing directories are checked for file
/// creation directly, while missing cache, store, or global bin directories
/// probe subdirectory and file creation at their deepest existing ancestor
/// without leaving empty directories on disk.
/// When another user can modify an ancestor directory, fall back to a
/// read-only check rather than creating probe paths there.
pub(super) fn can_write_to_dir(dir: &Path) -> bool {
    let Some(resolved) = resolve_existing_dir(dir) else {
        return false;
    };
    let path = match &resolved {
        ExistingDir::Target(existing) | ExistingDir::Ancestor(existing) => existing,
    };
    let Ok(canonical) = dunce::canonicalize(path) else {
        return false;
    };
    if !is_safe_probe_ancestor_chain(&canonical) {
        return can_access_writable_dir(&canonical);
    }
    match resolved {
        ExistingDir::Target(_) => can_create_file(&canonical),
        ExistingDir::Ancestor(_) => can_create_subdir(&canonical),
    }
}

fn can_create_file(dir: &Path) -> bool {
    let Ok(open_dir) = pnpm_fs::OpenDir::open(dir) else {
        return false;
    };
    let entry = random_probe_entry_name("write");
    if open_dir.create_child_file_new(&entry, b"").is_err() {
        return false;
    }
    let _ = open_dir.remove_child_file(&entry);
    true
}

fn can_create_subdir(ancestor: &Path) -> bool {
    let Ok(mut probe_dir) = ProbeDir::new(ancestor) else {
        return false;
    };
    probe_dir.write_new_file("write", b"").is_ok()
}

#[cfg(unix)]
fn can_access_writable_dir(dir: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt as _;

    let Ok(c_path) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c_path` is a valid NUL-terminated string for the duration of the call.
    unsafe {
        libc::faccessat(libc::AT_FDCWD, c_path.as_ptr(), libc::W_OK | libc::X_OK, libc::AT_EACCESS)
            == 0
    }
}

#[cfg(windows)]
fn can_access_writable_dir(dir: &Path) -> bool {
    windows_acl::can_access_windows_writable_dir(dir)
}

#[cfg(not(any(unix, windows)))]
fn can_access_writable_dir(_dir: &Path) -> bool {
    false
}

enum ExistingDir {
    Target(PathBuf),
    Ancestor(PathBuf),
}

fn existing_ancestor_dir(dir: &Path) -> Option<PathBuf> {
    match resolve_existing_dir(dir)? {
        ExistingDir::Target(path) | ExistingDir::Ancestor(path) => Some(path),
    }
}

enum PathEntryState {
    Dir,
    Missing,
    Blocked,
}

fn classify_path_entry(candidate: &Path) -> PathEntryState {
    match fs::symlink_metadata(candidate) {
        Ok(_) if fs::metadata(candidate).is_ok_and(|meta| meta.is_dir()) => PathEntryState::Dir,
        Ok(_) => PathEntryState::Blocked,
        Err(error) if error.kind() == io::ErrorKind::NotFound => PathEntryState::Missing,
        Err(_) => PathEntryState::Blocked,
    }
}

fn normalize_non_empty_path(dir: &Path) -> Option<PathBuf> {
    if dir.as_os_str().is_empty() {
        return None;
    }
    let absolute =
        if dir.is_absolute() { dir.to_path_buf() } else { std::env::current_dir().ok()?.join(dir) };
    Some(pnpm_fs::lexical_normalize(&absolute))
}

fn resolve_existing_dir(dir: &Path) -> Option<ExistingDir> {
    let normalized = normalize_non_empty_path(dir)?;
    for ancestor in normalized
        .ancestors()
        .filter(|path| !path.as_os_str().is_empty())
    {
        match classify_path_entry(ancestor) {
            PathEntryState::Dir if ancestor == normalized => {
                return Some(ExistingDir::Target(ancestor.to_path_buf()));
            }
            PathEntryState::Dir => {
                return Some(ExistingDir::Ancestor(ancestor.to_path_buf()));
            }
            PathEntryState::Missing => {}
            PathEntryState::Blocked => return None,
        }
    }
    None
}

pub(super) enum ProbeAncestor {
    Safe(PathBuf),
    Unsafe,
    Unavailable,
}

pub(super) fn resolve_probe_ancestor(dir: &Path) -> ProbeAncestor {
    let Some(ancestor) = existing_ancestor_dir(dir) else {
        return ProbeAncestor::Unavailable;
    };
    let Ok(canonical) = dunce::canonicalize(ancestor) else {
        return ProbeAncestor::Unavailable;
    };
    if is_safe_probe_ancestor_chain(&canonical) {
        ProbeAncestor::Safe(canonical)
    } else {
        ProbeAncestor::Unsafe
    }
}

#[cfg(unix)]
fn is_safe_probe_ancestor_chain(canonical_dir: &Path) -> bool {
    // SAFETY: `geteuid` takes no arguments and has no preconditions.
    let euid = unsafe { libc::geteuid() };
    canonical_dir
        .ancestors()
        .filter(|path| !path.as_os_str().is_empty())
        .all(|ancestor| is_safe_directory_entry(ancestor, euid))
}

#[cfg(windows)]
fn is_safe_probe_ancestor_chain(canonical_dir: &Path) -> bool {
    windows_acl::is_safe_windows_ancestor_chain(canonical_dir)
}

#[cfg(not(any(unix, windows)))]
fn is_safe_probe_ancestor_chain(_canonical_dir: &Path) -> bool {
    true
}

#[cfg(unix)]
fn is_safe_directory_entry(dir: &Path, euid: libc::uid_t) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    let Ok(meta) = fs::symlink_metadata(dir) else {
        return false;
    };
    if !meta.file_type().is_dir() {
        return false;
    }
    let uid = meta.uid();
    if uid != euid && uid != 0 {
        return false;
    }
    !is_unsafe_writable_dir_mode(meta.mode())
}

#[cfg(unix)]
pub(super) fn is_unsafe_writable_dir_mode(mode: u32) -> bool {
    (mode & 0o1000) == 0 && (mode & 0o022) != 0
}

fn random_probe_entry_name(prefix: &str) -> OsString {
    let mut bytes = [0u8; 16];
    let suffix = match getrandom::fill(&mut bytes) {
        Ok(()) => u128::from_ne_bytes(bytes),
        Err(_) => 0,
    };
    OsString::from(format!(".pnpm-{prefix}-{}-{suffix:032x}", pnpm_fs::process_id()))
}

pub(super) struct ProbeDir {
    parent: pnpm_fs::OpenDir,
    dir: Option<pnpm_fs::OpenDir>,
    #[cfg(unix)]
    raw_dir: Option<fs::File>,
    name: OsString,
    entries: Vec<OsString>,
}

impl ProbeDir {
    pub(super) fn new(safe_ancestor: &Path) -> io::Result<Self> {
        let parent = pnpm_fs::OpenDir::open(safe_ancestor)?;
        if parent.canonical_path()?.as_deref() != Some(safe_ancestor) {
            return Err(io::Error::other("probe ancestor path changed during open"));
        }
        let temp = tempfile::Builder::new().prefix(".pnpm-doctor-").tempdir_in(parent.path())?;
        let created_path = temp.keep();
        let Some(name) = created_path.file_name().map(OsStr::to_os_string) else {
            return Err(io::Error::other("probe directory missing file name"));
        };
        let Some(dir) = parent.open_child_dir(&name)? else {
            return Err(io::Error::other("probe path is not a real directory"));
        };
        #[cfg(unix)]
        let raw_dir = Some(open_verified_unix_child_dir(safe_ancestor, &name)?);
        Ok(Self {
            parent,
            dir: Some(dir),
            #[cfg(unix)]
            raw_dir,
            name,
            entries: Vec::with_capacity(4),
        })
    }

    pub(super) fn write_new_file(&mut self, prefix: &str, content: &[u8]) -> io::Result<PathBuf> {
        let dir =
            self.dir.as_ref().ok_or_else(|| io::Error::other("probe directory unavailable"))?;
        let entry = random_probe_entry_name(prefix);
        dir.create_child_file_new(&entry, content)?;
        let entry_path = dir.path().join(&entry);
        self.entries.push(entry);
        Ok(entry_path)
    }

    pub(super) fn register_entry(&mut self, prefix: &str) -> io::Result<PathBuf> {
        let dir =
            self.dir.as_ref().ok_or_else(|| io::Error::other("probe directory unavailable"))?;
        let entry = random_probe_entry_name(prefix);
        let entry_path = dir.path().join(&entry);
        self.entries.push(entry);
        Ok(entry_path)
    }

    #[cfg(test)]
    pub(super) fn invalidate_for_test(&mut self) {
        self.dir = None;
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
        }
        let _ = self.parent.remove_empty_child_dir(&self.name);
    }
}

#[cfg(unix)]
fn open_verified_unix_child_dir(safe_ancestor: &Path, child_name: &OsStr) -> io::Result<fs::File> {
    use std::os::{
        fd::{AsRawFd as _, FromRawFd as _, OwnedFd},
        unix::{
            ffi::OsStrExt as _,
            fs::{MetadataExt as _, OpenOptionsExt as _},
        },
    };

    let parent = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(safe_ancestor)?;
    let c_name = std::ffi::CString::new(child_name.as_bytes())?;
    // SAFETY: `c_name` is NUL-terminated and `parent` stays open for the call.
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            c_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
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
fn unlink_probe_entry_unix(raw_dir: &fs::File, entry: &OsStr) {
    use std::os::{fd::AsRawFd as _, unix::ffi::OsStrExt as _};

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
