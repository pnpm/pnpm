//! Path-manipulation helpers shared across [`crate::bin_resolver`] and
//! [`crate::shim`]. Kept private to the crate.

use std::path::{Component, Path, PathBuf};

/// Lexically resolve `.` and `..` in `path` without touching the filesystem.
///
/// `.` (CurDir) components are dropped. `..` (ParentDir) components pop
/// the previous component when one exists. Otherwise the rule depends
/// on whether `out` is already anchored:
///
/// - Anchored (`out` has a root or a Windows `Prefix`): drop the `..`.
///   Node's `path.resolve("/a/../../b")` returns `/b`, not `/../b`, so
///   the lexical normalisation has to match for `is_subdir` containment
///   checks (a `starts_with` against the package root) to agree with
///   pnpm. Without this branch, a path like `<pkg>/x/../../bin.js`
///   would normalise to `/../<pkg>/bin.js` and be rejected as outside
///   `<pkg>` even when it resolves back inside.
/// - Unanchored: push `..` so a leading `..` survives. Relative
///   targets like `../shared/cli` need this for
///   `shim::relative_path_from`.
///
/// Filesystem-free is the whole point: callers in `bin_resolver::is_subdir`
/// and `shim::relative_path_from` run the check before the target files
/// exist on disk, where `std::fs::canonicalize` cannot help. Mirrors pnpm's
/// `is-subdir`, which is also purely lexical (it uses Node's
/// `path.resolve`).
pub(crate) fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if !out.pop() && !is_anchored(&out) {
                    out.push("..");
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Whether `path` is anchored to a filesystem root: it either starts
/// with a Unix `/` root or a Windows `Prefix` (drive letter, UNC
/// share). Used by [`lexical_normalize`] to decide whether a `..` that
/// cannot pop should be materialised or dropped.
fn is_anchored(path: &Path) -> bool {
    path.has_root() || matches!(path.components().next(), Some(Component::Prefix(_)))
}
