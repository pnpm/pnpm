use pacquet_workspace_projects_graph::lexical_normalize;
use std::path::{Path, PathBuf};

/// Join `rel` onto `prefix` and lexically normalize the result,
/// collapsing `.` and resolving `..` without touching the filesystem.
///
/// Mirrors Node's `path.join(prefix, rel)` as used by upstream's
/// [`parseProjectSelector`](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-filter/src/parseProjectSelector.ts#L43):
/// segments are concatenated and then normalized, so an absolute `rel`
/// (e.g. the `{/pkg}` directory selector) *extends* `prefix` rather than
/// replacing it. Rust's [`Path::join`] would instead drop `prefix` for
/// an absolute `rel`, so leading separators are stripped first.
pub fn lexical_join(prefix: &Path, rel: &str) -> PathBuf {
    let rel = rel.trim_start_matches(['/', '\\']);
    lexical_normalize(&prefix.join(rel))
}
