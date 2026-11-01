//! A file an import materializes from, with the mode this install read.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// A source file of an import and, when this install has already read it,
/// the on-disk mode of its inode.
///
/// `mode` is filled by whoever read the inode's metadata anyway: the store
/// integrity check, or the store writer that found the blob already
/// present. The importer decides whether a store inode can be hardlinked
/// from it and reads the mode from disk only when it is `None`
/// (pnpm/pnpm#17543).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StoreFile {
    pub path: PathBuf,
    pub mode: Option<u32>,
}

impl StoreFile {
    /// A file whose mode this install has not read.
    #[must_use]
    pub const fn new(path: PathBuf) -> Self {
        Self { path, mode: None }
    }

    #[must_use]
    pub const fn with_mode(path: PathBuf, mode: Option<u32>) -> Self {
        Self { path, mode }
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
