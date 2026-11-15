use super::{
    CheckResult,
    probe_path::{ProbeAncestor, ProbeDir, resolve_probe_ancestor, write_exclusive_probe_file},
};
use pnpm_config::{Config, NodeLinker};
use pnpm_fs::{FsHardLink, FsReflink};
use std::{
    io,
    path::{Path, PathBuf},
};

/// Probe which link strategies work between the store and the project, since
/// that determines how packages land in `node_modules` and how fast an install
/// is: a reflink (copy-on-write) or hardlink is near-free, a plain copy is not.
pub(super) fn check_filesystem_capabilities<Sys>(config: &Config) -> CheckResult
where
    Sys: FsReflink + FsHardLink,
{
    match create_probe_workspaces(config) {
        Ok(probes) => check_filesystem_capabilities_in_dirs::<Sys>(probes.as_dirs()),
        Err(ProbeWorkspaceError::UnsafeAncestor) => CheckResult::warn(
            "Filesystem",
            "cannot safely probe link support in a directory writable by other users",
            "Check ownership and permissions on the store and project ancestor directories.",
        ),
        Err(ProbeWorkspaceError::Unavailable) => CheckResult::warn(
            "Filesystem",
            "could not create a probe directory",
            "Check that the store and project directories are writable.",
        ),
    }
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

enum ProbeWorkspaceError {
    UnsafeAncestor,
    Unavailable,
}

struct ProbeWorkspaces {
    store: ProbeDir,
    link_target: ProbeDir,
    symlink_target: Option<ProbeDir>,
}

impl ProbeWorkspaces {
    fn as_dirs(&self) -> ProbeDirs<'_> {
        ProbeDirs {
            store: self.store.path(),
            link_target: self.link_target.path(),
            symlink_target: self.symlink_target
                .as_ref()
                .map(ProbeDir::path)
                .unwrap_or_else(|| self.link_target.path()),
        }
    }
}

fn create_probe_workspaces(config: &Config) -> Result<ProbeWorkspaces, ProbeWorkspaceError> {
    let (store_ancestor, link_ancestor, symlink_ancestor) = resolve_workspace_ancestors(config)?;
    let store = ProbeDir::new(&store_ancestor).map_err(|_| ProbeWorkspaceError::Unavailable)?;
    let link_target = ProbeDir::new(&link_ancestor).map_err(|_| ProbeWorkspaceError::Unavailable)?;
    let symlink_target = (symlink_ancestor != link_ancestor)
        .then(|| ProbeDir::new(&symlink_ancestor))
        .transpose()
        .map_err(|_| ProbeWorkspaceError::Unavailable)?;
    Ok(ProbeWorkspaces { store, link_target, symlink_target })
}

fn resolve_workspace_ancestors(
    config: &Config,
) -> Result<(PathBuf, PathBuf, PathBuf), ProbeWorkspaceError> {
    let store = resolve_probe_ancestor(config.store_dir.root());
    let link = resolve_probe_ancestor(link_target_dir(config));
    let symlink = resolve_probe_ancestor(&config.modules_dir);
    match (store, link, symlink) {
        (ProbeAncestor::Unavailable, _, _)
        | (_, ProbeAncestor::Unavailable, _)
        | (_, _, ProbeAncestor::Unavailable) => Err(ProbeWorkspaceError::Unavailable),
        (ProbeAncestor::Unsafe, _, _)
        | (_, ProbeAncestor::Unsafe, _)
        | (_, _, ProbeAncestor::Unsafe) => Err(ProbeWorkspaceError::UnsafeAncestor),
        (ProbeAncestor::Safe(store), ProbeAncestor::Safe(link), ProbeAncestor::Safe(symlink)) => {
            Ok((store, link, symlink))
        }
    }
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
    write_exclusive_probe_file(&source, b"pnpm-doctor")?;
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
