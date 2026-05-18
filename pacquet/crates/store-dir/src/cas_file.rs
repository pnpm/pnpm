use crate::{FileHash, StoreDir};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_fs::{
    EnsureFileError, ensure_file, ensure_parent_dir,
    file_mode::{EXEC_MODE, is_executable},
};
use sha2::{Digest, Sha512};
use std::path::PathBuf;

impl StoreDir {
    /// Path to a file in the store directory.
    pub fn cas_file_path(&self, hash: FileHash, executable: bool) -> PathBuf {
        let hex = format!("{hash:x}");
        let suffix = if executable { "-exec" } else { "" };
        self.file_path_by_hex_str(&hex, suffix)
    }

    /// Path to a content-addressed file given its pre-computed hex digest
    /// (from the SQLite store index) and its POSIX mode. Matches pnpm's
    /// [`getFilePathByModeInCafs`](https://github.com/pnpm/pnpm/blob/1819226b51/store/cafs/src/getFilePathInCafs.ts)
    /// so index entries written by either tool resolve to the same path.
    ///
    /// Returns `None` when `hex` is too short or not ASCII-hex.
    ///
    /// We require *more* than two hex chars — the first two become the
    /// shard directory `files/XX/`, and the rest is the file component.
    /// A two-char input produces an empty tail, which on disk is the
    /// shard directory itself (usually present), so without this tighter
    /// check a caller would hand a directory path back as if it were a
    /// CAFS file path. The ASCII-hex requirement additionally guards the
    /// `hex[..2]` slice inside `file_path_by_hex_str` from panicking on
    /// non-UTF-8-char-boundary input.
    pub fn cas_file_path_by_mode(&self, hex: &str, mode: u32) -> Option<PathBuf> {
        if hex.len() <= 2 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        // Same executable-bit rule the write side uses
        // (`pacquet_fs::file_mode::is_executable`, matching pnpm's
        // `modeIsExecutable`), so a blob written as `-exec` is read back
        // as `-exec` and vice versa. Using a raw `0o111` literal here
        // silently diverged from the write side for modes like `0o744`
        // and turned every lookup of such a file into a cache miss.
        let suffix = if is_executable(mode) { "-exec" } else { "" };
        Some(self.file_path_by_hex_str(hex, suffix))
    }
}

/// Error type of [`StoreDir::write_cas_file`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum WriteCasFileError {
    WriteFile(EnsureFileError),
}

impl StoreDir {
    /// Write a file from an npm package to the store directory.
    pub fn write_cas_file(
        &self,
        buffer: &[u8],
        executable: bool,
    ) -> Result<(PathBuf, FileHash), WriteCasFileError> {
        let file_hash = Sha512::digest(buffer);
        let file_path = self.cas_file_path(file_hash, executable);
        let mode = executable.then_some(EXEC_MODE);

        // Ensure the shard directory (`files/XX/`) exists. The CAS has
        // 256 shards keyed by `file_hash[0]`; `create_dir_all` does a
        // `stat` syscall every call even when the directory is already
        // there, so remember which shards we've created and skip on
        // repeat. Duplicate mkdirs across threads are benign — the first
        // few writes into a fresh shard may each call `create_dir_all`,
        // which is idempotent; once any of them completes and inserts
        // into the cache, subsequent writes take the fast path.
        let shard_byte = file_hash[0];
        if !self.shard_already_ensured(shard_byte) {
            let parent = file_path.parent().expect("CAS file path always has a parent shard dir");
            ensure_parent_dir(parent).map_err(WriteCasFileError::WriteFile)?;
            self.mark_shard_ensured(shard_byte);
        }

        ensure_file(&file_path, buffer, mode).map_err(WriteCasFileError::WriteFile)?;
        Ok((file_path, file_hash))
    }
}

#[cfg(test)]
mod tests;
