//! Which kind of directory link works on a Windows network share.
//!
//! No kind works everywhere. A junction is followed by the server, and a
//! Windows file server can't resolve a target written as the client sees it
//! (a mapped drive letter or a UNC path), while other servers handle such a
//! junction. A symlink is followed by the client, which may refuse one on a
//! share that points into a share ("remote to remote" in `fsutil behavior
//! query SymlinkEvaluation`, disabled by default), and creating one needs a
//! privilege. So the first link written on a share tries both next to it and
//! keeps the kind that can be opened.
//!
//! The trial links point at a directory beside them. A link whose target is
//! on another drive or share gets the kind found this way too.

use crate::mapped_drive_path::{NetworkShare, network_share_cached};
use derive_more::{Display, Error};
use std::{
    fs, io,
    path::Path,
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
    /// A junction can't be opened there, but a relative symlink can.
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

/// The kind of link to write at `link`, or `None` when it isn't on a network
/// share. The answer is found once per share and process by [`probe`], in
/// the directory that will hold `link`. An error means that directory could
/// not be probed, `NotFound` when it doesn't exist yet, and nothing is cached.
pub(super) fn share_links(link: &Path) -> io::Result<Option<ShareLinks>> {
    static KNOWN: RwLock<Vec<(NetworkShare, ShareLinks)>> = RwLock::new(Vec::new());
    static PROBING: Mutex<()> = Mutex::new(());

    let Some(share) = network_share_cached(link) else {
        return Ok(None);
    };
    let known = |share: &NetworkShare| {
        KNOWN
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(known, _)| known == share)
            .map(|(_, links)| links.clone())
    };
    if let Some(links) = known(&share) {
        return Ok(Some(links));
    }
    // One probe at a time, so parallel link writers wait for the first
    // answer instead of each probing the share.
    let _probing = PROBING.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(links) = known(&share) {
        return Ok(Some(links));
    }
    let links = probe(link.parent().unwrap_or_else(|| Path::new("")))?;
    KNOWN
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .push((share, links.clone()));
    Ok(Some(links))
}

/// Find the kind of link that can be opened in `dir` by creating one of
/// each to a sibling directory, in a scratch directory that is removed again.
pub(super) fn probe(dir: &Path) -> io::Result<ShareLinks> {
    static PROBE_ID: AtomicU64 = AtomicU64::new(0);

    let scratch = dir.join(format!(
        ".pnpm-link-probe-{}-{}",
        std::process::id(),
        PROBE_ID.fetch_add(1, Ordering::Relaxed),
    ));
    fs::create_dir(&scratch)?;
    let result = probe_in(&scratch);
    for entry in ["junction", "symlink", "target"] {
        let _ = fs::remove_dir(scratch.join(entry));
    }
    let _ = fs::remove_dir(&scratch);
    result
}

fn probe_in(scratch: &Path) -> io::Result<ShareLinks> {
    let target = scratch.join("target");
    fs::create_dir(&target)?;

    let junction = scratch.join("junction");
    let junction_error = match junction::create(super::windows::junction_target(&target), &junction)
        .and_then(|()| open_through(&junction))
    {
        Ok(()) => return Ok(ShareLinks::Junction),
        Err(error) => error,
    };
    let symlink = scratch.join("symlink");
    let symlink_error = match std::os::windows::fs::symlink_dir("target", &symlink)
        .and_then(|()| open_through(&symlink))
    {
        Ok(()) => return Ok(ShareLinks::Symlink),
        Err(error) => error,
    };
    Ok(ShareLinks::Unusable(UnusableShareLinksError {
        junction: junction_error.to_string(),
        symlink: symlink_error.to_string(),
    }))
}

/// Open the directory `link` points at, which is what loading a package
/// through the link does.
fn open_through(link: &Path) -> io::Result<()> {
    fs::read_dir(link).map(drop)
}
