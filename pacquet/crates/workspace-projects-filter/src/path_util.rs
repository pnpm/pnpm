use pacquet_workspace_projects_graph::lexical_normalize;
use std::path::{Path, PathBuf};

/// Join `rel` onto `prefix` and lexically normalize the result,
/// collapsing `.` and resolving `..` without touching the filesystem.
///
/// Mirrors Node's `path.join(prefix, rel)` (and `path.resolve` when
/// `prefix` is absolute) closely enough for directory selectors: an
/// absolute `rel` replaces `prefix`, a `..` pops the previous segment.
pub fn lexical_join(prefix: &Path, rel: &str) -> PathBuf {
    lexical_normalize(&prefix.join(rel))
}
