use crate::{FileHash, StoreDir};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_fs::{
    EnsureFileError, cas_write_lock, create_exclusive_temp_file, ensure_file, ensure_parent_dir,
    file_mode::{EXEC_MODE, is_executable},
    rename_with_retry,
};
use sha2::{Digest, Sha512};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

impl StoreDir {
    /// Path to a file in the store directory.
    pub fn cas_file_path(&self, hash: FileHash, executable: bool) -> PathBuf {
        // Sha-512 → 64 bytes → 128 hex chars. Render into a stack
        // buffer so the per-file path build doesn't pay a String
        // allocation just for the digest. `write!` of `{:02x}` is
        // identical on the wire to `format!("{hash:x}")` for the
        // `digest::Output<Sha512>` value (a fixed-size byte array
        // whose `LowerHex` impl emits each byte as `%02x`).
        use std::io::Write as _;
        let mut hex_buf = [0u8; 128];
        let mut writer = &mut hex_buf[..];
        for byte in &hash {
            // `write!` on `&mut [u8]` never errors when the buffer is
            // large enough — 128 bytes is exactly the digest length.
            write!(writer, "{byte:02x}").expect("hex buffer sized for full sha-512 digest");
        }
        let hex = std::str::from_utf8(&hex_buf).expect("LowerHex of byte digest is ASCII");
        let suffix = if executable { "-exec" } else { "" };
        self.file_path_by_hex_str(hex, suffix)
    }

    /// Path to a content-addressed file given its pre-computed hex digest
    /// (from the `SQLite` store index) and its POSIX mode. Uses the same
    /// CAFS path layout pnpm does, so index entries written by either
    /// tool resolve to the same path.
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
        // (`pnpm_fs::file_mode::is_executable`), so a blob written as
        // `-exec` is read back as `-exec` and vice versa.
        let suffix = if is_executable(mode) { "-exec" } else { "" };
        Some(self.file_path_by_hex_str(hex, suffix))
    }
}

/// Error type of [`StoreDir::write_cas_file`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum WriteCasFileError {
    WriteFile(EnsureFileError),
}

/// Error type of [`StoreDir::write_cas_file_from_reader`]. Splits the
/// reader side from the store side so the caller can attribute a
/// mid-stream failure to its actual origin — a tar/gzip decode error is
/// not a store-write error.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum WriteCasFileFromReaderError {
    /// The source reader failed while its bytes were being streamed in.
    Read(io::Error),
    /// The content-addressed store rejected the write.
    Write(WriteCasFileError),
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

        self.ensure_shard_dir(&file_path, file_hash[0])?;

        ensure_file(&file_path, buffer, mode).map_err(WriteCasFileError::WriteFile)?;
        Ok((file_path, file_hash))
    }

    /// Stream a file from an npm package into the store directory
    /// without buffering it in memory.
    ///
    /// The content hash — and therefore the CAS path — is only known
    /// once the reader is exhausted, so the bytes stream into an
    /// exclusively-created temp file in the `files/` directory (same
    /// filesystem as the shards, so the final rename never crosses a
    /// mount) while the SHA-512 is computed incrementally, and the temp
    /// file is renamed to its content-addressed path at the end. Peak
    /// memory is one fixed copy buffer regardless of file size.
    ///
    /// Returns the CAS path, the content hash, and the streamed byte
    /// count. A reader that yields the same bytes as a `buffer` handed
    /// to [`StoreDir::write_cas_file`] lands at the same path with the
    /// same guarantees: an existing regular file at the target is kept
    /// as the live entry only after a byte-compare against the streamed
    /// content, and anything else is atomically replaced.
    ///
    /// When `expected_size` is given, a reader that yields any other
    /// number of bytes fails with the `Read` variant *before* anything
    /// reaches a content-addressed path — a truncated source (e.g. a
    /// cut-short archive) must not commit its partial content to the
    /// store, even though such a blob would be correctly addressed.
    pub fn write_cas_file_from_reader(
        &self,
        reader: &mut dyn Read,
        executable: bool,
        expected_size: Option<u64>,
    ) -> Result<(PathBuf, FileHash, u64), WriteCasFileFromReaderError> {
        let write_error =
            |error| WriteCasFileFromReaderError::Write(WriteCasFileError::WriteFile(error));
        let io_write_error = |file_path: &PathBuf, error| {
            write_error(EnsureFileError::WriteFile { file_path: file_path.clone(), error })
        };

        let files_dir = self.files_dir();
        ensure_parent_dir(files_dir).map_err(write_error)?;
        let mode = executable.then_some(EXEC_MODE);
        let (tmp_path, file) =
            create_exclusive_temp_file(files_dir, "stream", mode).map_err(write_error)?;

        // Bytes arrive in decompressor-sized chunks (tens of KB);
        // BufWriter coalesces them so the kernel sees fewer, larger
        // writes.
        let mut writer = io::BufWriter::with_capacity(COPY_BUFFER_SIZE, file);
        let mut hasher = Sha512::new();
        let mut copy_buffer = vec![0u8; COPY_BUFFER_SIZE];
        let mut size: u64 = 0;
        let result = loop {
            match reader.read(&mut copy_buffer) {
                Ok(0) => break Ok(()),
                Ok(read) => {
                    hasher.update(&copy_buffer[..read]);
                    if let Err(error) = writer.write_all(&copy_buffer[..read]) {
                        break Err(io_write_error(&tmp_path, error));
                    }
                    size += read as u64;
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => break Err(WriteCasFileFromReaderError::Read(error)),
            }
        };
        let result = result.and_then(|()| {
            writer
                .into_inner()
                .map_err(|error| io_write_error(&tmp_path, error.into_error()))
                .map(drop)
        });
        if let Err(error) = result {
            let _ = fs::remove_file(&tmp_path);
            return Err(error);
        }
        if let Some(expected) = expected_size
            && size != expected
        {
            let _ = fs::remove_file(&tmp_path);
            return Err(WriteCasFileFromReaderError::Read(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("reader yielded {size} bytes where {expected} were expected"),
            )));
        }

        let file_hash = hasher.finalize();
        let file_path = self.cas_file_path(file_hash, executable);
        if let Err(error) = self.ensure_shard_dir(&file_path, file_hash[0]) {
            let _ = fs::remove_file(&tmp_path);
            return Err(WriteCasFileFromReaderError::Write(error));
        }

        // Serialize with buffer-based writers ([`ensure_file`]) and
        // verifiers of the same path, per [`cas_write_lock`]'s
        // coordination contract.
        let lock = cas_write_lock(&file_path);
        let _guard = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

        // A regular file already at the hash-derived target is kept —
        // preserving its inode — only after its bytes are verified
        // against the freshly streamed temp, the same guarantee
        // [`ensure_file`]'s byte-compare gives the buffered writer.
        // Anything else (missing, torn or corrupt blob, symlink or
        // other non-regular dirent) is atomically replaced by the
        // rename, which is self-healing in every such state.
        let existing_is_live = fs::symlink_metadata(&file_path)
            .is_ok_and(|meta| meta.file_type().is_file() && meta.len() == size)
            && files_have_equal_contents(&tmp_path, &file_path);
        if existing_is_live {
            let _ = fs::remove_file(&tmp_path);
            return Ok((file_path, file_hash, size));
        }
        if let Err(error) = rename_with_retry(&tmp_path, &file_path) {
            let _ = fs::remove_file(&tmp_path);
            return Err(write_error(EnsureFileError::RenameFile {
                tmp_path,
                file_path: file_path.clone(),
                error,
            }));
        }
        Ok((file_path, file_hash, size))
    }

    /// Ensure the shard directory (`files/XX/`) exists. The CAS has
    /// 256 shards keyed by `file_hash[0]`; `create_dir_all` does a
    /// `stat` syscall every call even when the directory is already
    /// there, so remember which shards we've created and skip on
    /// repeat. Duplicate mkdirs across threads are benign — the first
    /// few writes into a fresh shard may each call `create_dir_all`,
    /// which is idempotent; once any of them completes and inserts
    /// into the cache, subsequent writes take the fast path.
    fn ensure_shard_dir(&self, file_path: &Path, shard_byte: u8) -> Result<(), WriteCasFileError> {
        if !self.shard_already_ensured(shard_byte) {
            let parent = file_path.parent().expect("CAS file path always has a parent shard dir");
            ensure_parent_dir(parent).map_err(WriteCasFileError::WriteFile)?;
            self.mark_shard_ensured(shard_byte);
        }
        Ok(())
    }
}

/// Chunk size for [`StoreDir::write_cas_file_from_reader`]'s read loop
/// and its `BufWriter`. 128 KB keeps syscall count low without a
/// per-file allocation worth worrying about — the streaming path only
/// runs for large entries.
const COPY_BUFFER_SIZE: usize = 128 * 1024;

/// Stream-compare two files' contents without loading either into
/// memory. Any open or read failure counts as "not equal" — every
/// caller's recovery for inequality (an atomic rename over the target)
/// is safe in each such state, so distinguishing them buys nothing.
fn files_have_equal_contents(left: &Path, right: &Path) -> bool {
    use std::io::BufRead;

    let Ok(file_a) = fs::File::open(left) else { return false };
    let Ok(file_b) = fs::File::open(right) else { return false };
    let mut reader_a = io::BufReader::with_capacity(COPY_BUFFER_SIZE, file_a);
    let mut reader_b = io::BufReader::with_capacity(COPY_BUFFER_SIZE, file_b);
    loop {
        let Ok(chunk_a) = reader_a.fill_buf() else { return false };
        let Ok(chunk_b) = reader_b.fill_buf() else { return false };
        if chunk_a.is_empty() || chunk_b.is_empty() {
            return chunk_a.is_empty() && chunk_b.is_empty();
        }
        let len = chunk_a.len().min(chunk_b.len());
        if chunk_a[..len] != chunk_b[..len] {
            return false;
        }
        reader_a.consume(len);
        reader_b.consume(len);
    }
}

#[cfg(test)]
mod tests;
