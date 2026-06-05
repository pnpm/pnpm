use std::path::Path;

use crate::lexical_normalize;

/// Whether `child` lexically resolves to a path under `parent`.
///
/// Mirrors npm's [`is-subdir`](https://github.com/zkochan/packages/blob/main/is-subdir/index.js)
/// — the same helper pnpm uses for guards like
/// [`isSubdir(opts.lockfileDir, linkedDependency.resolution.directory)`](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-resolver/src/index.ts#L235).
/// Filesystem-free: both paths are lexically normalised
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
