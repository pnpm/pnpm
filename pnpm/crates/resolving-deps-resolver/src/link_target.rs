//! Re-anchor workspace `link:` targets in lockfile-root-relative space.
//!
//! A `link:` target is stored relative to one anchor (the lockfile root
//! or the consuming importer's directory) and repeatedly re-expressed
//! against the other, once per dependency edge. Doing that math on the
//! absolute paths walks and re-allocates the anchors' long common
//! prefix on every edge; both anchors hang off the same lockfile root,
//! so the prefix always cancels out. The helpers here run the same
//! component math on the root-relative suffixes instead.
//!
//! Every helper is a *fast path*: it returns `None` for any input where
//! the relative-space result is not trivially the same as the
//! absolute-space one — a dot-carrying anchor or target, an importer
//! outside the lockfile root, an absolute target — and the caller falls
//! back to the absolute-space math. Within the guarded domain (a clean
//! absolute lockfile root, an importer on a clean path under it) the
//! two computations agree component-for-component, because
//! `diff_paths` and `lexical_normalize` are lexical: prefixing both
//! arguments with the same clean root never changes the outcome.

use std::path::{Component, Path, PathBuf};

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

/// Express a lockfile-root-relative `target` relative to the importer
/// at [`importer_rel_dir`]. `None` for a target that is absolute or
/// carries dot components.
pub(crate) fn target_relative_to_importer(
    target: &Path,
    importer_rel_dir: &Path,
) -> Option<PathBuf> {
    if !all_normal(target) {
        return None;
    }
    pathdiff::diff_paths(target, importer_rel_dir)
}

/// Express an importer-relative `target` (which typically climbs out
/// of the importer via `..` components) relative to the lockfile root.
/// `None` for a target that carries a root or prefix component — on
/// Windows a rooted `\abs` path is not `is_absolute`, yet `join` would
/// silently replace the base with it — or one that still escapes the
/// lockfile root after normalization.
pub(crate) fn target_relative_to_lockfile_root(
    target: &Path,
    importer_rel_dir: &Path,
) -> Option<PathBuf> {
    if !target
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::ParentDir | Component::CurDir))
    {
        return None;
    }
    let normalized = pnpm_fs::lexical_normalize(&importer_rel_dir.join(target));
    match normalized.components().next() {
        Some(Component::ParentDir) => None,
        _ => Some(normalized),
    }
}

fn is_clean_absolute(path: &Path) -> bool {
    // `is_absolute` carries the platform rules — on Windows it demands
    // a prefix *and* a root, rejecting drive-relative `C:foo` and
    // rootless `\foo` alike.
    path.is_absolute()
        && path.components().all(|c| !matches!(c, Component::CurDir | Component::ParentDir))
}

fn all_normal(path: &Path) -> bool {
    path.components().all(|c| matches!(c, Component::Normal(_)))
}

#[cfg(test)]
mod tests;
