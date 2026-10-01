use super::PatchCommitError;
#[cfg(target_family = "wasm")]
use pnpm_fs::temp_dir;
#[cfg(not(target_family = "wasm"))]
use std::env::temp_dir;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
const MAX_DIFF_OUTPUT_BYTES: u64 = 128 * 1024 * 1024;
pub(super) struct DiffTempFile {
    pub(super) path: PathBuf,
    pub(super) writer: File,
}

impl DiffTempFile {
    pub(super) fn new(stream: &'static str) -> Result<Self, PatchCommitError> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let pid = crate::process::id();
        let temp_dir = temp_dir();
        for _ in 0..16 {
            let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = temp_dir.join(format!("pacquet-git-diff-{stream}-{pid}-{counter}.tmp"));
            match create_diff_temp_file(&path) {
                Ok(writer) => return Ok(Self { path, writer }),
                Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
                Err(source) => return Err(PatchCommitError::DiffSpawn { source }),
            }
        }
        Err(PatchCommitError::DiffSpawn {
            source: io::Error::new(
                io::ErrorKind::AlreadyExists,
                "exhausted temp-path attempts for git diff output",
            ),
        })
    }

    pub(super) fn read_to_string(&self, stream: &'static str) -> Result<String, PatchCommitError> {
        let len = fs::metadata(&self.path)
            .map_err(|source| PatchCommitError::DiffSpawn { source })?
            .len();
        if len > MAX_DIFF_OUTPUT_BYTES {
            return Err(PatchCommitError::DiffOutputTooLarge {
                stream,
                limit: MAX_DIFF_OUTPUT_BYTES,
            });
        }
        let mut file =
            File::open(&self.path).map_err(|source| PatchCommitError::DiffSpawn { source })?;
        let mut bytes = Vec::with_capacity(len as usize);
        let mut buffer = [0; 8192];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|source| PatchCommitError::DiffSpawn { source })?;
            if read == 0 {
                break;
            }
            let next_len = bytes
                .len()
                .checked_add(read)
                .ok_or(PatchCommitError::DiffOutputTooLarge {
                    stream,
                    limit: MAX_DIFF_OUTPUT_BYTES,
                })?;
            if next_len as u64 > MAX_DIFF_OUTPUT_BYTES {
                return Err(PatchCommitError::DiffOutputTooLarge {
                    stream,
                    limit: MAX_DIFF_OUTPUT_BYTES,
                });
            }
            bytes.extend_from_slice(&buffer[..read]);
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

#[cfg(not(target_os = "wasi"))]
fn diff_temp_file_options() -> std::fs::OpenOptions {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    options
}

fn create_diff_temp_file(path: &Path) -> io::Result<File> {
    #[cfg(not(target_os = "wasi"))]
    {
        diff_temp_file_options().open(path)
    }
    #[cfg(target_os = "wasi")]
    {
        pnpm_fs::create_new_with_mode(path, 0o600)
    }
}

impl Drop for DiffTempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
