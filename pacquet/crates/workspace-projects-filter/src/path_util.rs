use std::path::{Component, Path, PathBuf};

/// Join `rel` onto `prefix` and lexically normalize the result,
/// collapsing `.` and resolving `..` without touching the filesystem.
///
/// Mirrors Node's `path.join(prefix, rel)` (and `path.resolve` when
/// `prefix` is absolute) closely enough for directory selectors: an
/// absolute `rel` replaces `prefix`, a `..` pops the previous segment.
pub fn lexical_join(prefix: &Path, rel: &str) -> PathBuf {
    lexical_normalize(&prefix.join(rel))
}

/// Collapse `.` and resolve `..` in `path` lexically.
pub fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}
