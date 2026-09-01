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
/// Filesystem-free: callers run this against paths whose targets may
/// not exist yet, where [`std::fs::canonicalize`] cannot help.
#[must_use]
pub fn lexical_normalize(path: &Path) -> PathBuf {
    // Always rebuilt component-by-component, never copied verbatim:
    // besides the dot components, the rebuild also strips trailing and
    // doubled separators, and callers compare and hash the results.
    let mut kept: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => match kept.last() {
                Some(Component::Normal(_)) => {
                    kept.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => kept.push(Component::ParentDir),
            },
            Component::CurDir => {}
            other => kept.push(other),
        }
    }
    let mut out = PathBuf::with_capacity(path.as_os_str().len());
    for component in kept {
        out.push(component.as_os_str());
    }
    out
}

#[cfg(test)]
mod tests;
