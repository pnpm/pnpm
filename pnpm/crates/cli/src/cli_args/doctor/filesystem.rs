use super::CheckResult;
use pnpm_config::{Config, NodeLinker};
use pnpm_fs::{FsHardLink, FsReflink};
use std::{
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
pub(super) fn can_write_to_dir(dir: &Path) -> bool {
    match resolve_existing_dir(dir) {
        Some(ExistingDir::Target(existing)) => can_create_file(&existing),
        Some(ExistingDir::Ancestor(ancestor)) => can_create_subdir(&ancestor),
        None => false,
    }
}

fn can_create_file(dir: &Path) -> bool {
    pnpm_fs::private_named_tempfile_in(dir).is_ok()
}

fn can_create_subdir(ancestor: &Path) -> bool {
    let Ok(probe_dir) = make_probe_dir(ancestor) else {
        return false;
    };
    can_create_file(probe_dir.path())
}

enum ExistingDir {
    Target(PathBuf),
    Ancestor(PathBuf),
}

/// Return the deepest ancestor of `dir` (including `dir` itself) that already
/// exists on disk and resolves to a directory, or `None` when an ancestor is
/// blocked by a non-directory entry, a dangling symlink, or an unreadable parent.
pub(super) fn existing_ancestor_dir(dir: &Path) -> Option<PathBuf> {
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

/// Probe which link strategies work between the store and the project, since
/// that determines how packages land in `node_modules` and how fast an install
/// is: a reflink (copy-on-write) or hardlink is near-free, a plain copy is not.
pub(super) fn check_filesystem_capabilities<Sys>(config: &Config) -> CheckResult
where
    Sys: FsReflink + FsHardLink,
{
    let Ok(probes) = create_probe_workspaces(config) else {
        return CheckResult::warn(
            "Filesystem",
            "could not create a probe directory",
            "Check that the store and project directories are writable.",
        );
    };
    check_filesystem_capabilities_in_dirs::<Sys>(probes.as_dirs())
}

pub(super) fn check_filesystem_capabilities_in_dirs<Sys>(dirs: ProbeDirs<'_>) -> CheckResult
where
    Sys: FsReflink + FsHardLink,
{
    let title = "Filesystem";
    let Ok(capabilities) = probe_link_capabilities::<Sys>(dirs) else {
        return CheckResult::warn(
            title,
            "could not write a probe file",
            "Check that the store directory is writable.",
        );
    };

    let available = capabilities.available_names();
    let detail = if available.is_empty() {
        "only copying is available".to_owned()
    } else {
        format!("available: {}", available.join(", "))
    };
    if capabilities.has_cheap_link() {
        CheckResult::pass(title, detail)
    } else {
        CheckResult::warn(
            title,
            detail,
            "Neither reflink nor hardlink works between the store and this project; installs will copy files and be slower. Put the store on the same filesystem as your projects.",
        )
    }
}

struct ProbeWorkspaces {
    store: tempfile::TempDir,
    link_target: tempfile::TempDir,
    symlink_target: Option<tempfile::TempDir>,
}

impl ProbeWorkspaces {
    fn as_dirs(&self) -> ProbeDirs<'_> {
        ProbeDirs {
            store: self.store.path(),
            link_target: self.link_target.path(),
            symlink_target: self.symlink_target
                .as_ref()
                .map(tempfile::TempDir::path)
                .unwrap_or_else(|| self.link_target.path()),
        }
    }
}

fn create_probe_workspaces(config: &Config) -> io::Result<ProbeWorkspaces> {
    let store_ancestor = existing_ancestor_dir(config.store_dir.root())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "store ancestor unavailable"))?;
    let link_ancestor = existing_ancestor_dir(link_target_dir(config))
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "link target ancestor unavailable")
        })?;
    let symlink_ancestor = existing_ancestor_dir(&config.modules_dir)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "modules ancestor unavailable"))?;

    let store = make_probe_dir(&store_ancestor)?;
    let link_target = make_probe_dir(&link_ancestor)?;
    let symlink_target = (symlink_ancestor != link_ancestor)
        .then(|| make_probe_dir(&symlink_ancestor))
        .transpose()?;
    Ok(ProbeWorkspaces { store, link_target, symlink_target })
}

fn make_probe_dir(ancestor: &Path) -> io::Result<tempfile::TempDir> {
    tempfile::Builder::new().prefix(".pnpm-doctor-").tempdir_in(ancestor)
}

fn link_target_dir(config: &Config) -> &Path {
    match config.node_linker {
        NodeLinker::Hoisted => &config.modules_dir,
        NodeLinker::Isolated | NodeLinker::Pnp | NodeLinker::Loaded => config.virtual_store_dir(),
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ProbeDirs<'a> {
    pub(super) store: &'a Path,
    pub(super) link_target: &'a Path,
    pub(super) symlink_target: &'a Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LinkCapabilities {
    pub(super) reflink: bool,
    pub(super) hardlink: bool,
    pub(super) symlink: bool,
}

impl LinkCapabilities {
    pub(super) fn available_names(self) -> Vec<&'static str> {
        let mut names = Vec::with_capacity(3);
        if self.reflink {
            names.push("reflink");
        }
        if self.hardlink {
            names.push("hardlink");
        }
        if self.symlink {
            names.push("symlink");
        }
        names
    }

    pub(super) fn has_cheap_link(self) -> bool {
        self.reflink || self.hardlink
    }
}

pub(super) fn probe_link_capabilities<Sys>(dirs: ProbeDirs<'_>) -> io::Result<LinkCapabilities>
where
    Sys: FsReflink + FsHardLink,
{
    let source = dirs.store.join("source");
    fs::write(&source, "pnpm-doctor")?;
    Ok(LinkCapabilities {
        reflink: Sys::reflink(&source, &dirs.link_target.join("reflink")).is_ok(),
        hardlink: Sys::hard_link(&source, &dirs.link_target.join("hardlink")).is_ok(),
        symlink: symlink_file(&source, &dirs.symlink_target.join("symlink")).is_ok(),
    })
}

#[cfg(unix)]
fn symlink_file(source: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(source, link)
}

#[cfg(target_os = "wasi")]
fn symlink_file(source: &Path, link: &Path) -> io::Result<()> {
    pnpm_fs::create_symlink(source, link, false)
}

#[cfg(windows)]
fn symlink_file(source: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(source, link)
}
