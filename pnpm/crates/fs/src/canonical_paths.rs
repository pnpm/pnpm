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
/// - a plain component under a canonical directory costs one `lstat` the
///   first time that directory entry is seen and nothing after;
/// - `..` steps to the canonical parent, as the kernel does.
///
/// A symlinked or missing component, a non-directory followed by more
/// components, or a relative path is answered by [`std::fs::canonicalize`]
/// on the whole path, so every result and error matches it. On other
/// platforms every call goes to [`std::fs::canonicalize`].
///
/// What a cache has seen is not read again: drop it before the directories
/// it read can change.
#[derive(Default)]
#[cfg_attr(not(unix), expect(dead_code, reason = "only Unix resolves components"))]
pub struct CanonicalPaths {
    /// Each path as given, mapped to its canonical form and whether that is
    /// a directory, or to `None` when [`std::fs::canonicalize`] must answer.
    paths: DashMap<PathBuf, Option<(PathBuf, bool)>>,
    /// A canonical directory joined with one component, mapped to whether
    /// that is a directory, or to `None` when it is a symlink or cannot be
    /// read. Shared by every spelling of a path that reaches it.
    entries: DashMap<PathBuf, Option<bool>>,
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
        // `Path` ignores a trailing slash, which only a directory takes.
        let trailing_slash = path
            .as_os_str()
            .as_bytes()
            .ends_with(b"/");
        (is_dir || !trailing_slash).then_some(canonical)
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
                    let is_dir = self.plain_entry_is_dir(&canonical)?;
                    Some((canonical, is_dir))
                }
                Component::CurDir | Component::RootDir | Component::Prefix(_) => None,
            });
        self.paths.insert(path.to_path_buf(), resolved.clone());
        resolved
    }

    /// Whether `entry` is a directory, or `None` when it is a symlink or
    /// cannot be read.
    #[cfg(unix)]
    fn plain_entry_is_dir(&self, entry: &Path) -> Option<bool> {
        if let Some(known) = self.entries.get(entry) {
            return *known;
        }
        let is_dir = std::fs::symlink_metadata(entry)
            .ok()
            .filter(|metadata| !metadata.file_type().is_symlink())
            .map(|metadata| metadata.is_dir());
        self.entries.insert(entry.to_path_buf(), is_dir);
        is_dir
    }
}

#[cfg(test)]
mod tests;
