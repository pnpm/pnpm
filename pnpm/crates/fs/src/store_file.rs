//! A file an import materializes from, with the mode this install read.

use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::OnceLock,
};

/// A source file of an import and, once this install has read it, the
/// on-disk mode of its inode.
///
/// The mode is filled by whoever read the inode's metadata anyway: the
/// store integrity check, or the store writer. The importer decides
/// whether a store inode can be hardlinked from it and reads the mode from
/// disk only when it is unknown, through [`Self::mode_or_read`], which
/// keeps that read for every import sharing this value, such as the peer
/// variants of one package (pnpm/pnpm#17543).
///
/// Equality and hashing use the path alone: the mode is a cached property
/// of the file, not part of its identity.
#[derive(Debug, Clone)]
pub struct StoreFile {
    pub path: PathBuf,
    mode: OnceLock<u32>,
}

impl StoreFile {
    /// A file whose mode this install has not read.
    #[must_use]
    pub const fn new(path: PathBuf) -> Self {
        Self { path, mode: OnceLock::new() }
    }

    #[must_use]
    pub fn with_mode(path: PathBuf, mode: Option<u32>) -> Self {
        Self { path, mode: mode.map_or_else(OnceLock::new, OnceLock::from) }
    }

    /// The mode this install has read, if any.
    #[must_use]
    pub fn mode(&self) -> Option<u32> {
        self.mode.get().copied()
    }

    /// Record a mode read for this file, replacing any earlier one.
    pub fn set_mode(&mut self, mode: u32) {
        self.mode = OnceLock::from(mode);
    }

    /// The known mode, or the one `read` returns for [`Self::path`], which
    /// is then kept.
    pub fn mode_or_read(&self, read: impl FnOnce(&Path) -> io::Result<u32>) -> io::Result<u32> {
        if let Some(mode) = self.mode.get() {
            return Ok(*mode);
        }
        let mode = read(&self.path)?;
        Ok(*self.mode.get_or_init(|| mode))
    }
}

impl PartialEq for StoreFile {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for StoreFile {}

impl std::hash::Hash for StoreFile {
    fn hash<Hasher: std::hash::Hasher>(&self, state: &mut Hasher) {
        self.path.hash(state);
    }
}

impl From<PathBuf> for StoreFile {
    fn from(path: PathBuf) -> Self {
        Self::new(path)
    }
}

impl AsRef<Path> for StoreFile {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl std::ops::Deref for StoreFile {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

/// `in-package filename` → the file it is imported from.
pub type CasPaths = HashMap<String, StoreFile>;

/// `paths` as [`CasPaths`] whose modes are all unread.
#[must_use]
pub fn cas_paths_from_paths(paths: HashMap<String, PathBuf>) -> CasPaths {
    paths
        .into_iter()
        .map(|(filename, path)| (filename, StoreFile::new(path)))
        .collect()
}

#[cfg(test)]
mod tests;
