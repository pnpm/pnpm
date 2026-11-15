#[cfg(any(windows, test))]
pub(super) mod windows_acl;

pub(super) use probe_dir::ProbeDir;

use super::CheckResult;
use pnpm_config::Config;
#[cfg(not(unix))]
use probe_dir::create_exclusive_file_pinned;
use probe_dir::random_probe_entry_name;
#[cfg(unix)]
use probe_dir::{create_exclusive_file_unix, unlink_probe_entry_unix};
use std::{
    ffi::OsStr,
    fs, io,
    path::{Component, Path, PathBuf},
};

mod probe_dir;

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

#[cfg(not(windows))]
pub(super) fn prefetch_candidate_paths(_candidates: &[&Path]) {}

#[cfg(windows)]
pub(super) fn prefetch_candidate_paths(candidates: &[&Path]) {
    let mut dirs = Vec::new();
    for candidate in candidates {
        append_canonical_ancestors(candidate, &mut dirs);
    }
    windows_acl::prefetch_windows_dir_security(&dirs);
}

pub(super) fn prefetch_config_paths(config: &Config) {
    let candidates: Vec<&Path> = [
        config.global_bin_dir.as_deref(),
        config.global_dir.as_deref(),
        Some(config.cache_dir.as_path()),
        Some(config.store_dir.root()),
        Some(config.virtual_store_dir()),
        Some(config.modules_dir.as_path()),
    ]
    .into_iter()
    .flatten()
    .collect();
    prefetch_candidate_paths(&candidates);
}

#[cfg(windows)]
fn append_canonical_ancestors(candidate: &Path, dirs: &mut Vec<PathBuf>) {
    let Some(existing) = existing_ancestor_dir(candidate) else {
        return;
    };
    let Ok(canonical) = dunce::canonicalize(existing) else {
        return;
    };
    dirs.extend(
        canonical
            .ancestors()
            .filter(|ancestor| !ancestor.as_os_str().is_empty())
            .map(Path::to_path_buf),
    );
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

#[cfg(unix)]
fn can_create_file(dir: &Path) -> bool {
    use std::os::unix::fs::OpenOptionsExt as _;

    let Ok(raw_dir) = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(dir)
    else {
        return false;
    };
    let entry = random_probe_entry_name("write");
    if create_exclusive_file_unix(&raw_dir, &entry, b"").is_err() {
        return false;
    }
    unlink_probe_entry_unix(&raw_dir, &entry);
    true
}

#[cfg(not(unix))]
fn can_create_file(dir: &Path) -> bool {
    let Ok(open_dir) = pnpm_fs::OpenDir::open(dir) else {
        return false;
    };
    let entry = random_probe_entry_name("write");
    if create_exclusive_file_pinned(&open_dir, &entry, b"").is_err() {
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
    pnpm_ffi_fs::can_write_and_search(dir)
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

fn resolve_existing_dir(dir: &Path) -> Option<ExistingDir> {
    if dir.as_os_str().is_empty() {
        return None;
    }
    let absolute =
        if dir.is_absolute() { dir.to_path_buf() } else { std::env::current_dir().ok()?.join(dir) };
    let mut current = PathBuf::new();
    let mut missing_depth = 0usize;
    for component in absolute.components() {
        step_path_component(component, &mut current, &mut missing_depth)?;
    }
    if current.as_os_str().is_empty() {
        return None;
    }
    if missing_depth == 0 {
        Some(ExistingDir::Target(current))
    } else {
        Some(ExistingDir::Ancestor(current))
    }
}

fn step_path_component(
    component: Component<'_>,
    current: &mut PathBuf,
    missing_depth: &mut usize,
) -> Option<()> {
    match component {
        Component::Prefix(prefix) => current.push(prefix.as_os_str()),
        Component::RootDir => {
            current.push(component.as_os_str());
            if !matches!(classify_path_entry(current), PathEntryState::Dir) {
                return None;
            }
            *current = dunce::canonicalize(&*current).ok()?;
        }
        Component::CurDir => {}
        Component::ParentDir if *missing_depth > 0 => *missing_depth -= 1,
        Component::ParentDir => {
            if let Some(parent) = current
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
            {
                *current = parent.to_path_buf();
            }
        }
        Component::Normal(_) if *missing_depth > 0 => *missing_depth += 1,
        Component::Normal(segment) => step_normal_segment(segment, current, missing_depth)?,
    }
    Some(())
}

fn step_normal_segment(
    segment: &OsStr,
    current: &mut PathBuf,
    missing_depth: &mut usize,
) -> Option<()> {
    let candidate = current.join(segment);
    match classify_path_entry(&candidate) {
        PathEntryState::Dir => {
            *current = dunce::canonicalize(&candidate).ok()?;
            Some(())
        }
        PathEntryState::Missing => {
            *missing_depth = 1;
            Some(())
        }
        PathEntryState::Blocked => None,
    }
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
    let euid = pnpm_ffi_process::effective_user_id();
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
