use std::path::Path;

use crate::lexical_normalize;

/// Whether `child` lexically resolves to a path under `parent`.
///
/// Mirrors npm's [`is-subdir`](https://github.com/zkochan/packages/blob/main/is-subdir/index.js),
/// used to guard against linking dependencies outside the workspace
/// root. Filesystem-free: both paths are lexically normalised
/// (`.` / `..` collapsed) before the prefix check, so callers can
/// run this against targets that don't exist yet.
#[must_use]
pub fn is_subdir(parent: &Path, child: &Path) -> bool {
    let parent_norm = lexical_normalize(parent);
    let child_norm = lexical_normalize(child);
    child_norm.starts_with(&parent_norm)
}

#[cfg(test)]
mod tests;
