use super::CheckResult;
use std::{
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

const PROBE_ENTRY_NAMES: [&str; 4] = ["source", "reflink", "hardlink", "symlink"];

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
/// read-only `faccessat` check rather than creating probe paths there.
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
    pnpm_fs::private_named_tempfile_in(dir).is_ok()
}

fn can_create_subdir(ancestor: &Path) -> bool {
    let Ok(probe_dir) = ProbeDir::new(ancestor) else {
        return false;
    };
    can_create_file(probe_dir.path())
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

#[cfg(not(unix))]
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

#[cfg(not(unix))]
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
    !is_unsafe_writable_dir_mode(meta.mode(), uid, meta.gid(), euid)
}

#[cfg(unix)]
fn is_unsafe_writable_dir_mode(
    mode: u32,
    uid: libc::uid_t,
    gid: libc::gid_t,
    euid: libc::uid_t,
) -> bool {
    if (mode & 0o1000) != 0 {
        return false;
    }
    if (mode & 0o002) != 0 {
        return true;
    }
    if (mode & 0o020) == 0 || gid == 0 {
        return false;
    }
    !(euid != 0 && uid == euid && gid == euid)
}

pub(super) struct ProbeDir {
    path: PathBuf,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
}

impl ProbeDir {
    pub(super) fn new(safe_ancestor: &Path) -> io::Result<Self> {
        let dir = tempfile::Builder::new().prefix(".pnpm-doctor-").tempdir_in(safe_ancestor)?;
        let path = dir.keep();
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;

            let meta = fs::symlink_metadata(&path)?;
            if !meta.file_type().is_dir() {
                return Err(io::Error::other("probe path is not a directory"));
            }
            Ok(Self { path, dev: meta.dev(), ino: meta.ino() })
        }
        #[cfg(not(unix))]
        {
            Ok(Self { path })
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    fn matches_identity(&self) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;

            fs::symlink_metadata(&self.path)
                .is_ok_and(|meta| {
                    meta.file_type().is_dir() && meta.dev() == self.dev && meta.ino() == self.ino
                })
        }
        #[cfg(not(unix))]
        {
            fs::symlink_metadata(&self.path).is_ok_and(|meta| meta.file_type().is_dir())
        }
    }
}

impl Drop for ProbeDir {
    fn drop(&mut self) {
        if !self.matches_identity() {
            return;
        }
        for name in PROBE_ENTRY_NAMES {
            let _ = fs::remove_file(self.path.join(name));
        }
        if self.matches_identity() {
            let _ = fs::remove_dir(&self.path);
        }
    }
}

pub(super) fn write_exclusive_probe_file(path: &Path, content: &[u8]) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;

        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(path)?;
    file.write_all(content)
}
