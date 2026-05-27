use std::path::{Component, Path, PathBuf};

/// Lexically normalize a path: collapse `.` and resolve `..` without
/// touching the filesystem. Mirrors the `path.resolve` step taken before
/// directory-based lookups (edge resolution here, and directory
/// selectors in `pacquet-workspace-projects-filter`); the on-disk slow
/// path for case-insensitive filesystems is intentionally omitted.
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
