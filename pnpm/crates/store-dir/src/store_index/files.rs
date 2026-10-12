//! The per-package-file backend of [`StoreIndex`](super::StoreIndex).
//!
//! Every row lives in its own file under `<store-root>/package-index/`,
//! written to a temporary name and renamed into place. Readers and writers
//! need no locks, so the index stays consistent on a filesystem that several
//! machines mount at once, where `SQLite`'s WAL locking does not work. Two
//! processes that update the same row at once each write a whole row, and
//! the later rename wins.

use super::StoreIndexError;
use pnpm_fs::{create_exclusive_temp_file, ensure_parent_dir, rename_with_retry};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    io::Write,
    path::{Path, PathBuf},
};

/// Directory under the store root that holds a per-package-file index. Its
/// presence is what selects this backend: every process opening the store
/// sees the same layout, whatever its own configuration says.
pub const PACKAGE_INDEX_DIR: &str = "package-index";

/// Prefix of the temporary files an entry is written to before the rename.
/// Scans skip it, so a temporary file left by a crashed writer is never
/// read as a row.
const TEMP_FILE_PREFIX: &str = ".tmp";

/// Byte length of the key-length header in front of each entry.
const KEY_LEN_SIZE: usize = size_of::<u32>();

/// A `package-index/` directory of a store.
pub(super) struct FilesIndex {
    dir: PathBuf,
}

impl FilesIndex {
    /// The index of the store at `store_root`, if the store uses this
    /// backend.
    pub(super) fn detect(store_root: &Path) -> Option<FilesIndex> {
        let dir = store_root.join(PACKAGE_INDEX_DIR);
        dir.is_dir()
            .then_some(FilesIndex { dir })
    }

    /// Switch the store at `store_root` to this backend.
    pub(super) fn create(store_root: &Path) -> Result<(), StoreIndexError> {
        let dir = store_root.join(PACKAGE_INDEX_DIR);
        pnpm_fs::file_mode::create_dir_all_inheriting_mode(&dir)
            .map_err(|source| StoreIndexError::CreateDir { path: dir, source })
    }

    /// The file of `key`, sharded by the first byte of the key's SHA-256 like
    /// the content-addressed files are.
    fn entry_path(&self, key: &str) -> PathBuf {
        let hex = format!("{:x}", Sha256::digest(key.as_bytes()));
        self.dir.join(&hex[..2]).join(&hex[2..])
    }

    /// The value stored under `key`. An entry that fails to decode, or that
    /// belongs to another key, reads as missing, like an undecodable
    /// `SQLite` row.
    pub(super) fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StoreIndexError> {
        let path = self.entry_path(key);
        let Some(bytes) = read_entry_file(&path)? else { return Ok(None) };
        Ok(decode_entry(&bytes)
            .filter(|(entry_key, _)| *entry_key == key)
            .map(|(_, value)| value.to_vec()))
    }

    pub(super) fn get_many(
        &self,
        keys: &[String],
    ) -> Result<Vec<(String, Vec<u8>)>, StoreIndexError> {
        // Reads on a network filesystem are dominated by round trips, which
        // run in parallel.
        keys.par_iter()
            .filter_map(|key| match self.get(key) {
                Ok(value) => value.map(|value| Ok((key.clone(), value))),
                Err(error) => Some(Err(error)),
            })
            .collect()
    }

    pub(super) fn contains(&self, key: &str) -> Result<bool, StoreIndexError> {
        let path = self.entry_path(key);
        path.try_exists()
            .map_err(|source| StoreIndexError::ReadEntry { path, source })
    }

    pub(super) fn contains_many(&self, keys: &[String]) -> Result<Vec<String>, StoreIndexError> {
        keys.par_iter()
            .filter_map(|key| match self.contains(key) {
                Ok(true) => Some(Ok(key.clone())),
                Ok(false) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }

    /// Write `value` under `key`, replacing an existing entry atomically:
    /// a concurrent reader sees either the old entry or the new one.
    pub(super) fn set(&self, key: &str, value: &[u8]) -> Result<(), StoreIndexError> {
        let path = self.entry_path(key);
        let shard = path.parent().expect("an entry path has a shard directory");
        ensure_parent_dir(shard).map_err(|source| StoreIndexError::CreateEntry { source })?;
        let (temp_path, mut file) = create_exclusive_temp_file(shard, TEMP_FILE_PREFIX, None)
            .map_err(|source| StoreIndexError::CreateEntry { source })?;
        let written = file.write_all(&encode_entry(key, value));
        drop(file);
        if let Err(source) = written.and_then(|()| rename_with_retry(&temp_path, &path)) {
            let _ = fs::remove_file(&temp_path);
            return Err(StoreIndexError::WriteEntry { path, source });
        }
        Ok(())
    }

    /// Write every entry, continuing past a failed one. Returns the first
    /// error.
    pub(super) fn set_many(&self, entries: &[(String, Vec<u8>)]) -> Result<(), StoreIndexError> {
        let errors: Vec<StoreIndexError> = entries
            .par_iter()
            .filter_map(|(key, value)| self.set(key, value).err())
            .collect();
        errors
            .into_iter()
            .next()
            .map_or(Ok(()), Err)
    }

    pub(super) fn remove(&self, key: &str) -> Result<(), StoreIndexError> {
        let path = self.entry_path(key);
        match fs::remove_file(&path) {
            Err(source) if source.kind() != io::ErrorKind::NotFound => {
                Err(StoreIndexError::WriteEntry { path, source })
            }
            _ => Ok(()),
        }
    }

    /// Visit every entry. An entry removed while the scan runs is skipped,
    /// and so is one that fails to decode.
    pub(super) fn for_each<VisitError>(
        &self,
        mut visit: impl FnMut(String, Vec<u8>) -> Result<(), VisitError>,
    ) -> Result<(), VisitError>
    where
        VisitError: From<StoreIndexError>,
    {
        for shard in read_dir_paths(&self.dir)? {
            if !shard.is_dir() {
                continue;
            }
            for path in read_dir_paths(&shard)? {
                let is_temp = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_none_or(|name| name.starts_with(TEMP_FILE_PREFIX));
                if is_temp {
                    continue;
                }
                let Some(bytes) = read_entry_file(&path)? else { continue };
                if let Some((key, value)) = decode_entry(&bytes) {
                    visit(key.to_string(), value.to_vec())?;
                }
            }
        }
        Ok(())
    }
}

fn read_entry_file(path: &Path) -> Result<Option<Vec<u8>>, StoreIndexError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(StoreIndexError::ReadEntry { path: path.to_path_buf(), source }),
    }
}

fn read_dir_paths(dir: &Path) -> Result<Vec<PathBuf>, StoreIndexError> {
    let to_error = |source| StoreIndexError::ReadEntry { path: dir.to_path_buf(), source };
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(to_error(source)),
    };
    entries
        .map(|entry| entry.map(|entry| entry.path()).map_err(to_error))
        .collect()
}

/// An entry is the key's byte length as a little-endian `u32`, the key,
/// and the value. The key is stored because the file name is its hash.
fn encode_entry(key: &str, value: &[u8]) -> Vec<u8> {
    let key_len = u32::try_from(key.len()).expect("a store-index key is shorter than 4 GiB");
    let mut bytes = Vec::with_capacity(KEY_LEN_SIZE + key.len() + value.len());
    bytes.extend_from_slice(&key_len.to_le_bytes());
    bytes.extend_from_slice(key.as_bytes());
    bytes.extend_from_slice(value);
    bytes
}

fn decode_entry(bytes: &[u8]) -> Option<(&str, &[u8])> {
    let (key_len, rest) = bytes.split_first_chunk::<KEY_LEN_SIZE>()?;
    let key_len = usize::try_from(u32::from_le_bytes(*key_len)).ok()?;
    let (key, value) = rest.split_at_checked(key_len)?;
    Some((std::str::from_utf8(key).ok()?, value))
}

#[cfg(test)]
mod tests;
