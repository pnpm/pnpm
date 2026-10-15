use super::{Backend, INDEX_DIR, StoreIndex, StoreIndexError, files::FilesIndex};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// How a store keeps its package index: the `storeIndexFormat` setting.
///
/// The setting only decides the format of a store that has no per-file
/// index yet. A store that has one keeps using it (see [`StoreIndex`]).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StoreIndexFormat {
    /// One `SQLite` database, `index.db`.
    #[default]
    Sqlite,
    /// One file per package under [`INDEX_DIR`].
    Files,
}

impl StoreIndex {
    pub(super) fn detect_files(store_dir: &Path) -> Option<Self> {
        FilesIndex::detect(store_dir).map(|index| StoreIndex { backend: Backend::Files(index) })
    }

    /// Whether the store at `store_dir` has an index to read, in either
    /// format.
    #[must_use]
    pub fn exists(store_dir: &Path) -> bool {
        store_dir.join("index.db").exists() || store_dir.join(INDEX_DIR).is_dir()
    }

    /// Give the store at `store_dir` a per-file index if `format` asks for
    /// one and the store has none yet. An existing per-file index is kept
    /// whatever `format` says.
    pub fn apply_format(store_dir: &Path, format: StoreIndexFormat) -> Result<(), StoreIndexError> {
        match format {
            StoreIndexFormat::Files => FilesIndex::create(store_dir),
            StoreIndexFormat::Sqlite => Ok(()),
        }
    }
}
