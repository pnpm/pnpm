//! The per-package-file backend of [`StoreIndex`](super::StoreIndex).
//!
//! Every row lives in its own file under `<store-root>/index/`,
//! written to a temporary name and renamed into place. Readers and writers
//! need no locks, so the index stays consistent on a filesystem that several
//! machines mount at once, where `SQLite`'s WAL locking does not work. Two
//! processes that update the same row at once each write a whole row, and
//! the later rename wins.

use super::StoreIndexError;
use pnpm_fs::{EnsureFileError, create_exclusive_temp_file, ensure_parent_dir, rename_with_retry};
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
pub const INDEX_DIR: &str = "index";

/// Directory under the store root that holds the aliases of a per-package-file
/// index.
const ALIAS_DIR: &str = "index-aliases";

/// Prefix of the temporary files an entry is written to before the rename.
/// Scans skip it, so a temporary file left by a crashed writer is never
/// read as a row.
const TEMP_FILE_PREFIX: &str = ".tmp";

/// Byte length of the key-length header in front of each entry.
const KEY_LEN_SIZE: usize = size_of::<u32>();

/// An `index/` directory of a store.
#[derive(Clone)]
pub(super) struct FilesIndex {
    dir: PathBuf,
}

impl FilesIndex {
    /// The index of the store at `store_root`, if the store uses this
    /// backend.
    pub(super) fn detect(store_root: &Path) -> Option<FilesIndex> {
        let dir = store_root.join(INDEX_DIR);
        dir.is_dir()
            .then_some(FilesIndex { dir })
    }

    /// Switch the store at `store_root` to this backend.
    pub(super) fn create(store_root: &Path) -> Result<(), StoreIndexError> {
        let dir = store_root.join(INDEX_DIR);
        pnpm_fs::file_mode::create_dir_all_inheriting_mode(&dir)
            .map_err(|source| StoreIndexError::CreateDir { path: dir, source })
    }

    /// The aliases of this index's rows, kept in a sibling directory so a
    /// scan of the rows never reads one.
    pub(super) fn aliases(&self) -> FilesIndex {
        FilesIndex { dir: self.dir.with_file_name(ALIAS_DIR) }
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

    /// [`Self::get`], reading the row `key` is an alias of when it has none
    /// of its own.
    pub(super) fn get_through_alias(&self, key: &str) -> Result<Option<Vec<u8>>, StoreIndexError> {
        if let Some(value) = self.get(key)? {
            return Ok(Some(value));
        }
        match self.alias_of(key)? {
            Some(target) => self.get(&target),
            None => Ok(None),
        }
    }

    /// [`Self::contains`], following an alias like [`Self::get_through_alias`].
    pub(super) fn contains_through_alias(&self, key: &str) -> Result<bool, StoreIndexError> {
        if self.contains(key)? {
            return Ok(true);
        }
        match self.alias_of(key)? {
            Some(target) => self.contains(&target),
            None => Ok(false),
        }
    }

    fn alias_of(&self, key: &str) -> Result<Option<String>, StoreIndexError> {
        Ok(self
            .aliases()
            .get(key)?
            .and_then(|target| String::from_utf8(target).ok()))
    }

    /// Whether [`Self::get`] finds an entry for `key`.
    pub(super) fn contains(&self, key: &str) -> Result<bool, StoreIndexError> {
        let Some(bytes) = read_entry_file(&self.entry_path(key))? else { return Ok(false) };
        Ok(decode_entry(&bytes).is_some_and(|(entry_key, _)| entry_key == key))
    }

    /// The value of the entry with the smallest key that `matches`. Every
    /// entry is read, in parallel.
    pub(super) fn first_matching(
        &self,
        matches: impl Fn(&str) -> bool + Sync,
    ) -> Result<Option<Vec<u8>>, StoreIndexError> {
        let found: Vec<(String, Vec<u8>)> = self
            .entry_paths()?
            .par_iter()
            .filter_map(|path| match read_entry_file(path) {
                Ok(bytes) => {
                    let (key, value) = decode_entry(bytes.as_deref()?)?;
                    matches(key).then(|| Ok((key.to_string(), value.to_vec())))
                }
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<_, _>>()?;
        Ok(found
            .into_iter()
            .min_by(|(left, _), (right, _)| left.cmp(right))
            .map(|(_, value)| value))
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
        let (temp_path, mut file) = create_temp_file_in_shard(shard)?;
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
        match errors.into_iter().next() {
            Some(error) => Err(error),
            None => Ok(()),
        }
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
        for path in self.entry_paths()? {
            let Some(bytes) = read_entry_file(&path)? else { continue };
            if let Some((key, value)) = decode_entry(&bytes) {
                visit(key.to_string(), value.to_vec())?;
            }
        }
        Ok(())
    }

    /// The files of every entry, leaving out temporary files.
    fn entry_paths(&self) -> Result<Vec<PathBuf>, StoreIndexError> {
        let mut paths = Vec::new();
        for shard in read_dir_paths(&self.dir)? {
            if shard.is_dir() {
                paths.extend(
                    read_dir_paths(&shard)?
                        .into_iter()
                        .filter(|path| !is_temp_file(path)),
                );
            }
        }
        Ok(paths)
    }
}

fn is_temp_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_none_or(|name| name.starts_with(TEMP_FILE_PREFIX))
}

/// Create a temporary file in `shard`, creating the shard only when it is
/// missing: on a network filesystem every directory check is a round trip.
fn create_temp_file_in_shard(shard: &Path) -> Result<(PathBuf, fs::File), StoreIndexError> {
    let to_error = |source| StoreIndexError::CreateEntry { source };
    match create_exclusive_temp_file(shard, TEMP_FILE_PREFIX, None) {
        Err(EnsureFileError::CreateFile { error, .. })
            if error.kind() == io::ErrorKind::NotFound =>
        {
            ensure_parent_dir(shard).map_err(to_error)?;
            create_exclusive_temp_file(shard, TEMP_FILE_PREFIX, None).map_err(to_error)
        }
        result => result.map_err(to_error),
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
