//! Port of pnpm's
//! [`getStorePath` / `storePathRelativeToHome`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L14-L78).
//!
//! When the user has not pinned `storeDir`, pnpm places the store on
//! the same volume as the project so the on-disk layout can use
//! hardlinks instead of cross-volume copies. The choice is:
//!
//! 1. If a file in the project root can be hardlinked into the
//!    user's pnpm home directory, use `<pnpm_home>/store` (the home
//!    store).
//! 2. Otherwise walk from the filesystem root toward the project,
//!    find the first directory that *does* accept the hardlink (the
//!    mount point), prefer that mount point's parent if it is also
//!    linkable, and return `<mount_point>/.pnpm-store`. If only the
//!    project folder itself is linkable, fall back to the home store
//!    — that's what pnpm does at
//!    [`index.ts:67-68`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L67-L68).
//!
//! Without this detection a developer with a separate
//! case-sensitive workspace volume (for example
//! `/Volumes/src/` on macOS) would have pacquet install into the
//! case-*insensitive* home volume (`~/Library/pnpm/store`).
//! Downstream tools that compare canonical paths get confused by the
//! mismatch — typescript-eslint, for instance, canonicalises its
//! parser cache against the home store and then can't find the same
//! source files in the case-sensitive TypeScript program loaded from
//! the workspace volume, so `eslint --fix` fails with a
//! "`TSConfig` does not include this file" error on every project file.
//!
//! The hardlink attempt itself is threaded through the
//! [`LinkProbe`] capability so tests can answer the linkability
//! question without touching disk. The production [`Host`] impl
//! performs the real link attempts via [`host_can_link_between_dirs`].
//!
//! [`pacquet_store_dir::STORE_VERSION`] (`"v11"`) is *not* appended in
//! this module; the path returned here is the un-suffixed base. Every
//! caller wraps the result in [`pacquet_store_dir::StoreDir::from`],
//! which appends the suffix in one place — mirroring pnpm's
//! [`getStorePath`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L39-L42)
//! `if (!endsWith(v11)) append(v11)` branch. Doing the join at
//! construction guarantees that everything pacquet exposes externally
//! (the `storeDir` written to `.modules.yaml`, the path printed by
//! `pacquet store path`, the NDJSON `context` log event) matches the
//! value pnpm produces, so switching between the two tools no longer
//! trips `ERR_PNPM_UNEXPECTED_STORE`.
//!
//! [`Host`]: crate::api::Host

use crate::api::LinkProbe;
use std::{
    fs,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Resolve where to place the default pnpm store given the `SmartDefault`
/// home-based path and the project root.
///
/// Returns `home_default` unchanged when the project's volume can be
/// reached via hardlink from `pnpm_home_dir`. Otherwise returns
/// `<mountpoint>/.pnpm-store` where `<mountpoint>` is the first
/// ancestor of `pkg_root` that accepts the hardlink (the volume mount
/// point), preferring the mountpoint's parent when it too is linkable.
/// Falls back to `home_default` whenever the algorithm cannot complete
/// — pnpm's
/// [`storePathRelativeToHome`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L45-L78)
/// makes the same conservative choice on any error in its `try` /
/// `catch`.
pub fn resolve_store_dir<Sys: LinkProbe>(
    home_default: PathBuf,
    pnpm_home_dir: &Path,
    pkg_root: &Path,
) -> PathBuf {
    let Ok(pkg_root) = fs::canonicalize(pkg_root) else {
        return home_default;
    };

    if Sys::can_link_between_dirs(&pkg_root, pnpm_home_dir) {
        return home_default;
    }

    let Some(mountpoint) = root_link_target::<Sys>(&pkg_root) else {
        return home_default;
    };

    // Walk one level up if that parent is also linkable. Mirrors
    // [`index.ts:60-64`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L60-L64):
    // some mounts expose a writable parent (e.g. `/Volumes` on macOS
    // is writable even though the actual mount is `/Volumes/src`).
    let mountpoint = match mountpoint.parent() {
        Some(parent) if parent != mountpoint && Sys::can_link_between_dirs(&pkg_root, parent) => {
            parent.to_path_buf()
        }
        _ => mountpoint,
    };

    // pnpm's [`index.ts:67-68`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L67-L68):
    // when linkability is confined to the project folder itself, the
    // mount-point fallback would put the store *inside* the project
    // — instead, defer to the home store.
    if mountpoint == pkg_root {
        return home_default;
    }

    mountpoint.join(".pnpm-store")
}

/// Find the volume mount point for the directory containing `existing`,
/// using `can_link_between_dirs` to test each ancestor. Port of pnpm's
/// [`rootLinkTarget`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L6-L21).
///
/// Returns `None` if no ancestor accepts the hardlink (no usable
/// mount point) — the caller falls back to the home store.
fn root_link_target<Sys: LinkProbe>(pkg_root: &Path) -> Option<PathBuf> {
    let mut dir = filesystem_root(pkg_root);
    loop {
        if Sys::can_link_between_dirs(pkg_root, &dir) {
            return Some(dir);
        }
        if dir == pkg_root {
            return None;
        }
        dir = next_path(&dir, pkg_root);
    }
}

/// Extract the absolute-path prefix that anchors `path` (the root
/// `/` on Unix; the disk-prefix `C:\` on Windows; etc.). When `path`
/// has no absolute root component the function returns the path
/// unchanged — that branch should be unreachable here because
/// `pkg_root` is canonicalised before [`root_link_target`] is called.
fn filesystem_root(path: &Path) -> PathBuf {
    let mut root = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => root.push(component.as_os_str()),
            _ => break,
        }
    }
    if root.as_os_str().is_empty() { path.to_path_buf() } else { root }
}

/// Given `from` (an ancestor of `to`), return `from` with one more
/// path segment appended along the way to `to`. Port of npm's
/// [`next-path`](https://github.com/zkochan/packages/blob/main/next-path/index.js):
/// `nextPath('/', '/Volumes/src/proj')` → `'/Volumes'`,
/// `nextPath('/Volumes', '/Volumes/src/proj')` → `'/Volumes/src'`.
/// Returns `from` unchanged when `from` is not a prefix of `to`.
fn next_path(from: &Path, to: &Path) -> PathBuf {
    let from_components: Vec<_> = from.components().collect();
    let to_components: Vec<_> = to.components().collect();
    if to_components.len() <= from_components.len() {
        return from.to_path_buf();
    }
    for (i, c) in from_components.iter().enumerate() {
        if to_components.get(i) != Some(c) {
            return from.to_path_buf();
        }
    }
    let mut result = from.to_path_buf();
    if let Some(next) = to_components.get(from_components.len()) {
        result.push(next.as_os_str());
    }
    result
}

/// Real-filesystem implementation of [`LinkProbe::can_link_between_dirs`]
/// used by the production [`Host`][crate::api::Host] impl. Mirrors
/// pnpm's
/// [`canLinkToSubdir`](https://github.com/pnpm/pnpm/blob/29a42efc3b/store/path/src/index.ts#L80-L92):
/// create a temp file in `from_dir`, create a temp subdirectory in
/// `to_dir`, attempt the hardlink, then clean up. Failure for any
/// reason (parent missing, EACCES, EXDEV, ...) collapses to `false`.
pub(crate) fn host_can_link_between_dirs(from_dir: &Path, to_dir: &Path) -> bool {
    let src = path_temp_in(from_dir);
    if fs::File::create(&src).is_err() {
        return false;
    }
    let tmp_dir = path_temp_in(to_dir);
    if fs::create_dir_all(&tmp_dir).is_err() {
        let _ = fs::remove_file(&src);
        return false;
    }
    let dst = path_temp_in(&tmp_dir);
    let success = fs::hard_link(&src, &dst).is_ok();
    let _ = fs::remove_file(&dst);
    let _ = fs::remove_dir(&tmp_dir);
    let _ = fs::remove_file(&src);
    success
}

/// Generate a unique temp-path inside `folder`. Mirrors
/// [`pathTemp`](https://github.com/zkochan/packages/blob/main/path-temp/index.js):
/// `<folder>/_tmp_<pid>_<random>`. Collisions across racing pacquet
/// processes are avoided by encoding both the pid and a nanosecond
/// reading, which is sufficient because each callsite uses the path
/// once and removes it.
fn path_temp_in(folder: &Path) -> PathBuf {
    let pid = std::process::id();
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    folder.join(format!("_tmp_{pid}_{nanos:08x}"))
}

#[cfg(test)]
mod tests;
