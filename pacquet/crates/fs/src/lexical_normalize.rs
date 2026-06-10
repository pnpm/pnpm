use std::path::{Component, Path, PathBuf};

/// Lexically resolve `.` and `..` components without touching the
/// filesystem.
///
/// Mirrors Node's
/// [`path.join`](https://nodejs.org/api/path.html#pathjoinpaths) /
/// [`path.resolve`](https://nodejs.org/api/path.html#pathresolvepaths)
/// normalisation rules. Rust's [`Path::join`] alone does **not**
/// normalize — it appends segments verbatim — so callers that need
/// pnpm-compatible round-tripping of stored paths (e.g.
/// `node_modules/.modules.yaml`'s `virtualStoreDir` field, computed via
/// `path.relative` on write and `path.join` on read in pnpm) must run
/// the joined path through this helper to match upstream output.
///
/// Semantics:
/// - `foo/../bar` → `bar` (pop a real segment).
/// - `/..` → `/` (POSIX rule: root has no parent).
/// - `../foo` → `../foo` (preserve leading `..` in relative paths).
/// - `foo/./bar` → `foo/bar` (drop `.`).
///
/// Filesystem-free: callers run this against paths whose targets may
/// not exist yet, where [`std::fs::canonicalize`] cannot help.
#[must_use]
pub fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(".."),
            },
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests;
