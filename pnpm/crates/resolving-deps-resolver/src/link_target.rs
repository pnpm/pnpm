//! Re-anchor workspace `link:` targets in lockfile-root-relative space.
//!
//! A `link:` target is stored relative to one anchor (the lockfile root
//! or the consuming importer's directory) and repeatedly re-expressed
//! against the other, once per dependency edge. Doing that math on the
//! absolute paths walks and re-allocates the anchors' long common
//! prefix on every edge; both anchors hang off the same lockfile root,
//! so the prefix always cancels out. The helpers here run the same
//! component math on the root-relative suffixes instead — and since
//! both anchors are constant per importer, [`ImporterAnchor`] derives
//! the suffix once so the per-edge work is the suffix math alone.
//!
//! The anchor is a *fast path*: it disarms itself for any input where
//! the relative-space result is not trivially the same as the
//! absolute-space one — a dot-carrying anchor, a driveless-yet-rooted
//! Windows anchor, an importer outside the lockfile root — and each
//! rendering falls back to the caller's absolute-space math the same
//! way for an absolute or escaping target. Within the guarded domain
//! the two computations agree component-for-component, because
//! `diff_paths` and `lexical_normalize` are lexical: prefixing both
//! arguments with the same clean root never changes the outcome.

use std::path::{Component, Path, PathBuf};

/// The importer-side inputs of `link:` re-anchoring, derived once per
/// importer: its directory as a clean relative suffix of the lockfile
/// root. The workspace root importer holds the empty suffix.
#[derive(Debug, Default, Clone)]
pub(crate) struct ImporterAnchor {
    /// `None` — an anchor carries dot components, is not truly
    /// absolute, or the importer is not under the lockfile root — turns
    /// every rendering into the caller's fallback.
    rel_dir: Option<PathBuf>,
}

impl ImporterAnchor {
    pub(crate) fn new(project_dir: &Path, lockfile_dir: &Path) -> Self {
        ImporterAnchor {
            rel_dir: importer_rel_dir(project_dir, lockfile_dir).map(Path::to_path_buf),
        }
    }

    /// Express a lockfile-root-relative `target` relative to the
    /// importer. `None` for a disarmed anchor, or a target that is
    /// absolute or carries dot components.
    pub(crate) fn target_relative_to_importer(&self, target: &Path) -> Option<PathBuf> {
        let rel_dir = self.rel_dir.as_deref()?;
        if !all_normal(target) {
            return None;
        }
        pathdiff::diff_paths(target, rel_dir)
    }

    /// Express an importer-relative `target` (which typically climbs
    /// out of the importer via `..` components) relative to the
    /// lockfile root. `None` for a disarmed anchor, a target that
    /// carries a root or prefix component — on Windows a rooted `\abs`
    /// path is not `is_absolute`, yet `join` would silently replace the
    /// base with it — or one that still escapes the lockfile root after
    /// normalization.
    pub(crate) fn target_relative_to_lockfile_root(&self, target: &Path) -> Option<PathBuf> {
        let rel_dir = self.rel_dir.as_deref()?;
        if !target.components().all(|component| {
            matches!(component, Component::Normal(_) | Component::ParentDir | Component::CurDir)
        }) {
            return None;
        }
        let normalized = pnpm_fs::lexical_normalize(&rel_dir.join(target));
        match normalized.components().next() {
            Some(Component::ParentDir) => None,
            _ => Some(normalized),
        }
    }
}

/// The importer's directory as a clean relative suffix of the lockfile
/// root. `None` — an anchor carries `.` / `..` components, or the
/// importer is not under the root — sends the caller to the
/// absolute-space math. The workspace root importer yields the empty
/// path.
pub(crate) fn importer_rel_dir<'dir>(
    project_dir: &'dir Path,
    lockfile_dir: &Path,
) -> Option<&'dir Path> {
    if !is_clean_absolute(lockfile_dir) {
        return None;
    }
    let rel = project_dir.strip_prefix(lockfile_dir).ok()?;
    all_normal(rel).then_some(rel)
}

fn is_clean_absolute(path: &Path) -> bool {
    // `is_absolute` carries the platform rules — on Windows it demands
    // a prefix *and* a root, rejecting drive-relative `C:foo` and
    // rootless `\foo` alike.
    path.is_absolute()
        && path
            .components()
            .all(|component| !matches!(component, Component::CurDir | Component::ParentDir))
}

fn all_normal(path: &Path) -> bool {
    path.components().all(|component| matches!(component, Component::Normal(_)))
}

#[cfg(test)]
mod tests;
