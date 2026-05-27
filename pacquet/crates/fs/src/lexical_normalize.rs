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
mod tests {
    use super::lexical_normalize;
    use std::path::Path;

    #[test]
    fn collapses_parent_dir_segments() {
        assert_eq!(lexical_normalize(Path::new("foo/bar/../baz")), Path::new("foo/baz"));
    }

    #[test]
    fn drops_parent_dir_at_root() {
        assert_eq!(lexical_normalize(Path::new("/..")), Path::new("/"));
        assert_eq!(lexical_normalize(Path::new("/../foo")), Path::new("/foo"));
    }

    #[test]
    fn preserves_leading_parent_dir_when_unanchored() {
        assert_eq!(lexical_normalize(Path::new("../foo")), Path::new("../foo"));
        assert_eq!(lexical_normalize(Path::new("../../foo")), Path::new("../../foo"));
    }

    #[test]
    fn drops_current_dir_segments() {
        assert_eq!(lexical_normalize(Path::new("foo/./bar")), Path::new("foo/bar"));
        assert_eq!(lexical_normalize(Path::new("./foo")), Path::new("foo"));
    }

    #[test]
    fn collapses_unanchored_absolute_join() {
        let modules_dir = Path::new("/private/tmp/pkg/node_modules");
        let stored_relative = Path::new("../../../../Users/zoltan/Library/pnpm/store/v11/links");
        let joined = modules_dir.join(stored_relative);
        assert_eq!(
            lexical_normalize(&joined),
            Path::new("/Users/zoltan/Library/pnpm/store/v11/links"),
        );
    }

    #[test]
    fn empty_path_is_empty() {
        assert_eq!(lexical_normalize(Path::new("")), Path::new(""));
    }
}
