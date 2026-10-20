use dashmap::DashMap;
use std::{
    io,
    path::{Path, PathBuf},
};

/// [`std::fs::canonicalize`] for many paths that share their leading
/// directories, such as the `link:` targets of every workspace project.
///
/// On Unix, a path is resolved from its parent, and every path resolved is
/// remembered:
/// - a component under a canonical directory is canonicalized on its own
///   the first time any path reaches that directory entry and read from the
///   cache after, so a symlink is followed and each component is spelled as
///   [`std::fs::canonicalize`] spells it;
/// - `..` steps to the canonical parent, as the kernel does.
///
/// A component that cannot be read, a non-directory followed by more
/// components or by a trailing `/` or `/.`, or a relative path is answered
/// by [`std::fs::canonicalize`] on the whole path, so every result and error
/// matches it. On other platforms every call goes to
/// [`std::fs::canonicalize`].
///
/// What a cache has seen is not read again: drop it before the directories
/// it read can change.
#[derive(Default)]
#[cfg_attr(not(unix), expect(dead_code, reason = "only Unix resolves components"))]
pub struct CanonicalPaths {
    /// Each path resolved so far, as given or as a canonical directory joined
    /// with one component, mapped to its canonical form and whether that is
    /// a directory, or to `None` when [`std::fs::canonicalize`] must answer.
    paths: DashMap<PathBuf, Option<(PathBuf, bool)>>,
}

impl CanonicalPaths {
    /// See [`CanonicalPaths`].
    pub fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        #[cfg(unix)]
        if let Some(canonical) = self.resolve_unix(path) {
            return Ok(canonical);
        }
        std::fs::canonicalize(path)
    }

    #[cfg(unix)]
    fn resolve_unix(&self, path: &Path) -> Option<PathBuf> {
        use std::os::unix::ffi::OsStrExt;

        if !path.is_absolute() {
            return None;
        }
        let (canonical, is_dir) = self.resolve(path)?;
        // `Path` ignores a trailing `/` or `/.`, which only a directory takes.
        let bytes = path.as_os_str().as_bytes();
        let names_a_directory = bytes.ends_with(b"/") || bytes.ends_with(b"/.");
        (is_dir || !names_a_directory).then_some(canonical)
    }

    /// The canonical form of the absolute `path` and whether it is a
    /// directory.
    #[cfg(unix)]
    fn resolve(&self, path: &Path) -> Option<(PathBuf, bool)> {
        use std::path::Component;

        let Some(parent) = path.parent() else {
            return Some((PathBuf::from("/"), true));
        };
        if let Some(known) = self.paths.get(path) {
            return known.clone();
        }
        let resolved = self
            .resolve(parent)
            .filter(|(_, parent_is_dir)| *parent_is_dir)
            .and_then(|(mut canonical, _)| match path.components().next_back()? {
                Component::ParentDir => {
                    canonical.pop();
                    Some((canonical, true))
                }
                Component::Normal(name) => {
                    canonical.push(name);
                    self.resolve_entry(&canonical)
                }
                Component::CurDir | Component::RootDir | Component::Prefix(_) => None,
            });
        self.paths.insert(path.to_path_buf(), resolved.clone());
        resolved
    }

    /// The canonical form of `entry`, a canonical directory joined with one
    /// component, and whether it is a directory.
    #[cfg(unix)]
    fn resolve_entry(&self, entry: &Path) -> Option<(PathBuf, bool)> {
        if let Some(known) = self.paths.get(entry) {
            return known.clone();
        }
        let resolved = std::fs::canonicalize(entry)
            .ok()
            .and_then(|canonical| {
                let is_dir = std::fs::metadata(&canonical).ok()?.is_dir();
                Some((canonical, is_dir))
            });
        self.paths.insert(entry.to_path_buf(), resolved.clone());
        resolved
    }
}

#[cfg(test)]
mod tests;
