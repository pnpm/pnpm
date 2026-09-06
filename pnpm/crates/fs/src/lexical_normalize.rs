use std::{
    ffi::{OsStr, OsString},
    path::{Component, MAIN_SEPARATOR_STR, Path, PathBuf},
};

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
    // Concatenated by hand rather than through `PathBuf::push`: on Windows,
    // `push` takes a component that looks like a drive prefix (`C:tools`,
    // legal in the middle of a path) for the start of a new path and
    // discards everything before it.
    let mut out = OsString::with_capacity(path.as_os_str().len());
    let mut needs_separator = false;
    for component in normalize_components(path.components()) {
        match component {
            Component::Prefix(prefix) => out.push(prefix.as_os_str()),
            Component::RootDir => out.push(MAIN_SEPARATOR_STR),
            component => {
                if needs_separator {
                    out.push(MAIN_SEPARATOR_STR);
                }
                out.push(component.as_os_str());
                needs_separator = true;
            }
        }
    }
    PathBuf::from(out)
}

/// Normalize a POSIX path lexically on every platform, preserving a trailing
/// slash and returning `.` for an empty result. Backslashes remain literal.
#[must_use]
pub fn lexical_normalize_posix(path: &str) -> String {
    let root = path.starts_with('/').then_some(Component::RootDir);
    let components = path.split('/').filter(|part| !part.is_empty()).map(|part| match part {
        "." => Component::CurDir,
        ".." => Component::ParentDir,
        _ => Component::Normal(OsStr::new(part)),
    });
    let mut normalized = String::with_capacity(path.len());
    for component in normalize_components(root.into_iter().chain(components)) {
        if component == Component::RootDir {
            normalized.push('/');
        } else {
            if !normalized.is_empty() && !normalized.ends_with('/') {
                normalized.push('/');
            }
            normalized
                .push_str(component.as_os_str().to_str().expect("components came from UTF-8"));
        }
    }
    if normalized.is_empty() {
        normalized.push('.');
    }
    if path.ends_with('/') && !normalized.ends_with('/') {
        normalized.push('/');
    }
    normalized
}

fn normalize_components<'path>(
    components: impl Iterator<Item = Component<'path>>,
) -> Vec<Component<'path>> {
    let mut kept = Vec::new();
    for component in components {
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
    kept
}

#[cfg(test)]
mod tests;
