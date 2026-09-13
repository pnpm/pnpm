use super::{
    DocumentWrite, ErrorKind, PathBuf, RegistryError, Result, Store, fs, read_dir_if_present,
    record_key, write_atomic, write_atomic_new,
};

impl Store {
    /// A record's path. The key's `/` separators become path components, so a
    /// key never rides into a path as one string on a platform that would read
    /// it differently.
    pub(crate) fn record_path(&self, namespace: &str, key: &str) -> PathBuf {
        let mut path = self.root.join(namespace);
        path.extend(key.split('/'));
        path
    }

    pub(crate) async fn read_record(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>> {
        match fs::read(self.record_path(namespace, key)).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub(crate) async fn create_record(
        &self,
        namespace: &str,
        key: &str,
        bytes: &[u8],
    ) -> Result<bool> {
        match write_atomic_new(&self.record_path(namespace, key), bytes).await {
            Ok(()) => Ok(true),
            Err(RegistryError::Io(err)) if err.kind() == ErrorKind::AlreadyExists => Ok(false),
            Err(err) => Err(err),
        }
    }

    pub(crate) async fn replace_record_if_current(
        &self,
        namespace: &str,
        key: &str,
        expected: &[u8],
        bytes: &[u8],
    ) -> Result<DocumentWrite> {
        let _guard = self.record_write_lock.lock().await;
        if self.read_record(namespace, key).await?.as_deref() != Some(expected) {
            return Ok(DocumentWrite::Conflict);
        }
        write_atomic(&self.record_path(namespace, key), bytes).await?;
        Ok(DocumentWrite::Written)
    }

    /// Under the record-write lock, so a removal cannot land between a
    /// conditional replace's comparison and its write and see the record it
    /// deleted written back.
    pub(crate) async fn remove_record(&self, namespace: &str, key: &str) -> Result<bool> {
        let _guard = self.record_write_lock.lock().await;
        match fs::remove_file(self.record_path(namespace, key)).await {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    pub(crate) async fn list_record_keys(&self, namespace: &str) -> Result<Vec<String>> {
        let root = self.root.join(namespace);
        let mut keys = Vec::new();
        let mut pending = vec![(root, String::new())];
        while let Some((dir, prefix)) = pending.pop() {
            let Some(mut entries) = read_dir_if_present(&dir).await? else {
                continue;
            };
            while let Some(entry) = entries.next_entry().await? {
                let key = record_key(&prefix, &entry.file_name());
                if entry.file_type().await?.is_dir() {
                    pending.push((entry.path(), key));
                } else {
                    keys.push(key);
                }
            }
        }
        Ok(keys)
    }
}
