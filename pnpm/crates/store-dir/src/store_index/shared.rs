use super::{
    Backend, PackageFilesIndex, SharedReadonlyStoreIndex, StoreIndex, StoreIndexError,
    decode_index_value, files::FilesIndex,
};

impl StoreIndex {
    /// [`StoreIndex::get`] through a shared handle.
    ///
    /// A per-file index is read after the lock is released: its rows are
    /// independent files, and on a network filesystem every read is a round
    /// trip that concurrent lookups should overlap rather than queue for.
    /// A poisoned lock reads as a miss.
    pub fn get_shared(
        index: &SharedReadonlyStoreIndex,
        key: &str,
    ) -> Result<Option<PackageFilesIndex>, StoreIndexError> {
        match StoreIndex::lock_or_detach(index, |index| index.get(key)) {
            Detached::Files(files) => files
                .get_through_alias(key)?
                .map(|bytes| decode_index_value(&bytes))
                .transpose(),
            Detached::Locked(result) => result.unwrap_or(Ok(None)),
        }
    }

    /// [`StoreIndex::contains_key`] through a shared handle, with the
    /// locking of [`StoreIndex::get_shared`].
    pub fn contains_shared(
        index: &SharedReadonlyStoreIndex,
        key: &str,
    ) -> Result<bool, StoreIndexError> {
        match StoreIndex::lock_or_detach(index, |index| index.contains_key(key)) {
            Detached::Files(files) => files.contains_through_alias(key),
            Detached::Locked(result) => result.unwrap_or(Ok(false)),
        }
    }

    /// Run `query` under the lock, or hand back a per-file index to query
    /// without it. `Locked(None)` means the lock is poisoned.
    fn lock_or_detach<Output>(
        index: &SharedReadonlyStoreIndex,
        query: impl FnOnce(&StoreIndex) -> Output,
    ) -> Detached<Output> {
        let Ok(guard) = index.lock() else { return Detached::Locked(None) };
        match &guard.backend {
            Backend::Files(files) => Detached::Files(files.clone()),
            Backend::Sqlite { .. } => Detached::Locked(Some(query(&guard))),
        }
    }
}

enum Detached<Output> {
    Files(FilesIndex),
    Locked(Option<Output>),
}

#[cfg(test)]
mod tests;
