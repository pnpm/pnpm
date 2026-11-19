//! Which kind of directory link works on a Windows network share.
//!
//! No kind works everywhere. A junction is followed by the server, and a
//! Windows file server can't resolve a target written as the client sees it
//! (a mapped drive letter or a UNC path), while other servers handle such a
//! junction. A symlink is followed by the client, which may refuse one on a
//! share that points into a share or onto a local drive ("remote to remote"
//! and "remote to local" in `fsutil behavior query SymlinkEvaluation`, both
//! disabled by default), and creating one needs a privilege. So the first
//! link written on a share to a target on a given drive or share tries both
//! and keeps the kind that can be opened.

use crate::{
    mapped_drive_path::{NetworkShare, network_share_cached},
    retry::{
        ERROR_DELETE_PENDING, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION, retry_fs_operation,
    },
};
use derive_more::{Display, Error};
use std::{
    ffi::OsString,
    fs, io,
    path::{Component, Path, PathBuf},
    sync::{
        Mutex, PoisonError, RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

/// The kind of link to write on one share.
#[derive(Debug, Clone)]
pub(super) enum ShareLinks {
    /// A junction can be opened there. pnpm 11 writes junctions everywhere
    /// on Windows, so this is tried first.
    Junction,
    /// A junction can't be opened there, but a symlink can.
    Symlink,
    /// Neither can be opened there.
    Unusable(UnusableShareLinksError),
}

/// Neither kind of link can be opened on a share. An install that went on
/// would finish with a `node_modules` nothing can load packages from.
#[derive(Debug, Display, Error, Clone)]
#[display(
    "Links created on this network share cannot be opened. A junction failed with \
     \"{junction}\" and a symlink failed with \"{symlink}\". Move the project to a local drive, \
     or set `nodeLinker: hoisted` in pnpm-workspace.yaml, which copies registry dependencies \
     into node_modules instead of linking them. Workspace packages are linked even with the \
     hoisted linker."
)]
pub(super) struct UnusableShareLinksError {
    pub(super) junction: String,
    pub(super) symlink: String,
}

/// The drive or share a link's target is on.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TargetVolume {
    Share(NetworkShare),
    /// The path prefix of a target that isn't on a network share, in upper case.
    Local(OsString),
}

/// The kind of link to write at `link` to `target`, or `None` when `link`
/// isn't on a network share. The answer is found once per process for each
/// share and drive or share of the target by [`probe`], in the directory
/// that will hold `link`. An error means no answer was found, and nothing
/// is cached: `NotFound` when that directory doesn't exist yet, or a file
/// lock that outlasted its retries.
pub(super) fn share_links(link: &Path, target: &Path) -> io::Result<Option<ShareLinks>> {
    type Key = (NetworkShare, TargetVolume);
    static KNOWN: RwLock<Vec<(Key, ShareLinks)>> = RwLock::new(Vec::new());
    static PROBING: Mutex<()> = Mutex::new(());

    let Some(share) = network_share_cached(link) else {
        return Ok(None);
    };
    let key = (share, target_volume(target));
    let known = |key: &Key| {
        KNOWN
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(known, _)| known == key)
            .map(|(_, links)| links.clone())
    };
    if let Some(links) = known(&key) {
        return Ok(Some(links));
    }
    // One probe at a time, so parallel link writers wait for the first
    // answer instead of each probing the share.
    let _probing = PROBING.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(links) = known(&key) {
        return Ok(Some(links));
    }
    let (share, target_volume) = &key;
    let target_dir = (*target_volume != TargetVolume::Share(share.clone())).then(|| {
        nearest_existing_dir(target)
    });
    let links = probe(link.parent().unwrap_or_else(|| Path::new("")), target_dir.as_deref())?;
    KNOWN
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .push((key, links.clone()));
    Ok(Some(links))
}

fn target_volume(target: &Path) -> TargetVolume {
    if let Some(share) = network_share_cached(target) {
        return TargetVolume::Share(share);
    }
    let absolute = std::path::absolute(target);
    let target = absolute.as_deref().unwrap_or(target);
    let prefix = match target.components().next() {
        Some(Component::Prefix(prefix)) => prefix.as_os_str().to_ascii_uppercase(),
        _ => OsString::new(),
    };
    TargetVolume::Local(prefix)
}

/// The closest directory at or above `target` that exists, since a link can
/// be written before its target.
fn nearest_existing_dir(target: &Path) -> PathBuf {
    let absolute = std::path::absolute(target).unwrap_or_else(|_| target.to_path_buf());
    absolute
        .ancestors()
        .find(|dir| dir.is_dir())
        .unwrap_or(&absolute)
        .to_path_buf()
}

/// Find the kind of link that can be opened in `dir` by creating one of
/// each in a scratch directory that is removed again. The links point at a
/// scratch directory in `target_dir`, or beside them when it is `None`.
pub(super) fn probe(dir: &Path, target_dir: Option<&Path>) -> io::Result<ShareLinks> {
    let scratch = create_scratch_dir(dir)?;
    let result = match target_dir {
        None => {
            let target = scratch.join("target");
            fs::create_dir(&target).and_then(|()| probe_in(&scratch, &target, Path::new("target")))
        }
        Some(target_dir) => create_scratch_dir(target_dir)
            .and_then(|target| {
                let result = probe_in(&scratch, &target, &target);
                let _ = fs::remove_dir(&target);
                result
            }),
    };
    for entry in ["junction", "symlink", "target"] {
        let _ = fs::remove_dir(scratch.join(entry));
    }
    let _ = fs::remove_dir(&scratch);
    result
}

fn create_scratch_dir(parent: &Path) -> io::Result<PathBuf> {
    static PROBE_ID: AtomicU64 = AtomicU64::new(0);

    let scratch = parent.join(format!(
        ".pnpm-link-probe-{}-{}",
        std::process::id(),
        PROBE_ID.fetch_add(1, Ordering::Relaxed),
    ));
    fs::create_dir(&scratch)?;
    Ok(scratch)
}

fn probe_in(scratch: &Path, target: &Path, symlink_contents: &Path) -> io::Result<ShareLinks> {
    let junction = scratch.join("junction");
    let junction_error = match try_link(&junction, |link| {
        junction::create(super::windows::junction_target(target), link)
    }) {
        Ok(()) => return Ok(ShareLinks::Junction),
        Err(error) => error,
    };
    let symlink = scratch.join("symlink");
    let symlink_error = match try_link(&symlink, |link| {
        std::os::windows::fs::symlink_dir(symlink_contents, link)
    }) {
        Ok(()) => return Ok(ShareLinks::Symlink),
        Err(error) => error,
    };
    if is_file_lock(&junction_error) {
        return Err(junction_error);
    }
    if is_file_lock(&symlink_error) {
        return Err(symlink_error);
    }
    Ok(ShareLinks::Unusable(UnusableShareLinksError {
        junction: junction_error.to_string(),
        symlink: symlink_error.to_string(),
    }))
}

/// Create a link with `create` and open through it, retrying while another
/// process, such as a virus scanner, holds the new entry open.
fn try_link(link: &Path, create: impl Fn(&Path) -> io::Result<()>) -> io::Result<()> {
    let mut created = false;
    retry_fs_operation(
        || {
            if !created {
                // A failed `junction::create` can leave the plain directory
                // it starts from, which would open like a working link.
                create(link).inspect_err(|_| drop(fs::remove_dir(link)))?;
                created = true;
            }
            open_through(link)
        },
        is_file_lock,
    )
}

/// A file lock held by another process, which says nothing about the share.
fn is_file_lock(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION | ERROR_DELETE_PENDING),
    )
}

/// Open the directory `link` points at, which is what loading a package
/// through the link does.
pub(super) fn open_through(link: &Path) -> io::Result<()> {
    fs::read_dir(link).map(drop)
}
