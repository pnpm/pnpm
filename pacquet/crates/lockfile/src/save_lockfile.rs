use crate::{
    Lockfile, serialize_yaml,
    yaml_documents::{YAML_DOCUMENT_SEPARATOR, YAML_DOCUMENT_START, extract_env_document},
};
use derive_more::{Display, Error};
use pacquet_diagnostics::miette::{self, Diagnostic};
use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Error when writing the lockfile to the filesystem.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum SaveLockfileError {
    #[display("Failed to get current_dir: {_0}")]
    #[diagnostic(code(pacquet_lockfile::current_dir))]
    CurrentDir(io::Error),

    #[display("Failed to serialize lockfile to YAML: {_0}")]
    #[diagnostic(code(pacquet_lockfile::serialize_yaml))]
    SerializeYaml(serde_json::Error),

    #[display("Failed to write lockfile content: {_0}")]
    #[diagnostic(code(pacquet_lockfile::write_file))]
    WriteFile(io::Error),

    #[display("Failed to create virtual-store directory {dir:?}: {error}")]
    #[diagnostic(code(pacquet_lockfile::create_dir))]
    CreateDir {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to remove existing current-lockfile at {path:?}: {error}")]
    #[diagnostic(code(pacquet_lockfile::remove_file))]
    RemoveFile {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to rename temp file {tmp:?} over {target:?}: {error}")]
    #[diagnostic(code(pacquet_lockfile::rename_file))]
    RenameFile {
        tmp: PathBuf,
        target: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}

/// Write an arbitrary lockfile-shaped value to the *wanted* lockfile at
/// `path` as pnpm-formatted YAML.
///
/// Used by the `afterAllResolved` pnpmfile hook, whose JSON result may carry
/// arbitrary keys the typed [`Lockfile`] cannot represent. `serde_json`'s
/// `preserve_order` feature keeps the key order produced by serializing the
/// [`Lockfile`] and appended by the hook, so the output matches the typed write
/// for unmodified lockfiles.
///
/// The env lockfile document (the config-dependency snapshot that the
/// env-installer writes as the *first* YAML document of `pnpm-lock.yaml`)
/// is preserved: if `path` already begins with one, it is re-prepended
/// ahead of the freshly serialized main document. Mirrors upstream's
/// `writeWantedLockfile`, which re-reads the env document and writes
/// `${YAML_DOCUMENT_START}${envDoc}${YAML_DOCUMENT_SEPARATOR}${mainDoc}`
/// at <https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/fs/src/write.ts#L73-L80>.
/// A lockfile with no env document round-trips byte-for-byte (no
/// leading `---`).
pub fn save_value_to_path<Document: serde::Serialize>(
    value: &Document,
    path: &Path,
) -> Result<(), SaveLockfileError> {
    let content = serialize_yaml::to_string(value).map_err(SaveLockfileError::SerializeYaml)?;
    let env_prefix = match fs::read_to_string(path) {
        Ok(existing) => extract_env_document(&existing)
            .map(|env| format!("{YAML_DOCUMENT_START}{env}{YAML_DOCUMENT_SEPARATOR}")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(SaveLockfileError::WriteFile(error)),
    };
    let output = match env_prefix {
        Some(prefix) => format!("{prefix}{content}"),
        None => content,
    };
    fs::write(path, output).map_err(SaveLockfileError::WriteFile)
}

impl Lockfile {
    /// Save lockfile to a specific path.
    pub fn save_to_path(&self, path: &Path) -> Result<(), SaveLockfileError> {
        save_value_to_path(self, path)
    }

    /// Save lockfile to `pnpm-lock.yaml` in the current directory.
    pub fn save_to_current_dir(&self) -> Result<(), SaveLockfileError> {
        let file_path =
            env::current_dir().map_err(SaveLockfileError::CurrentDir)?.join(Lockfile::FILE_NAME);
        self.save_to_path(&file_path)
    }

    /// Save the *current* lockfile under
    /// `<virtual_store_dir>/lock.yaml` at end-of-install. Mirrors
    /// upstream's `writeCurrentLockfile` at
    /// <https://github.com/pnpm/pnpm/blob/94240bc046/lockfile/fs/src/write.ts#L41-L51>:
    ///
    /// - When the lockfile is empty ([`Lockfile::is_empty`]) the
    ///   existing file is removed and no new content is written.
    ///   Mirrors upstream's `rimraf` short-circuit so an empty install
    ///   doesn't leave stale state behind.
    /// - Otherwise the directory is created if missing and the file
    ///   is written atomically: serialize → write next-to + rename.
    ///   The rename is the only step an observer can race against,
    ///   so a partial install will never leave a torn lockfile.
    pub fn save_current_to_virtual_store_dir(
        &self,
        virtual_store_dir: &Path,
    ) -> Result<(), SaveLockfileError> {
        let target = virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME);

        if self.is_empty() {
            match fs::remove_file(&target) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(SaveLockfileError::RemoveFile { path: target, error }),
            }
        } else {
            fs::create_dir_all(virtual_store_dir).map_err(|error| {
                SaveLockfileError::CreateDir { dir: virtual_store_dir.to_path_buf(), error }
            })?;
            let content =
                serialize_yaml::to_string(self).map_err(SaveLockfileError::SerializeYaml)?;
            write_atomic(&target, content.as_bytes())
        }
    }
}

/// Write `content` to `target` via a temp file in the same directory
/// followed by `rename`. The rename is atomic on Unix and replaces
/// in-place on Windows, so an observer never sees a torn file.
///
/// The temp file is opened with `O_CREAT | O_EXCL` (`create_new(true)`)
/// rather than `create + truncate`, so we never follow a symlink or
/// truncate a file an attacker (or a crashed prior install) pre-seeded
/// at our predicted temp path. On `AlreadyExists` we advance the
/// counter and try again, up to `MAX_TEMP_ATTEMPTS` times — matching
/// the hardening already in `pacquet_fs::ensure_file::write_atomic`
/// (per-call review on [#442](https://github.com/pnpm/pacquet/pull/442)).
fn write_atomic(target: &Path, content: &[u8]) -> Result<(), SaveLockfileError> {
    /// Sixteen fresh counter values is plenty — under benign
    /// conditions we never collide; under shared-store-across-
    /// containers the chance of 16 consecutive same-pid same-counter
    /// collisions is negligible.
    const MAX_TEMP_ATTEMPTS: usize = 16;

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let pid = std::process::id();
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let file_name = target
        .file_name()
        .map_or_else(|| String::from("lock.yaml"), |name| name.to_string_lossy().into_owned());

    let mut last_already_exists: Option<io::Error> = None;
    for _ in 0..MAX_TEMP_ATTEMPTS {
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = parent.join(format!(".{file_name}.{pid}.{counter}.tmp"));

        let mut file = match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                // Stale temp file or adversarial / concurrent pre-seed
                // at the colliding path. Don't touch whatever is there;
                // retry with a fresh counter.
                last_already_exists = Some(error);
                continue;
            }
            Err(error) => return Err(SaveLockfileError::WriteFile(error)),
        };

        if let Err(error) = file.write_all(content) {
            drop(file);
            let _ = fs::remove_file(&tmp);
            return Err(SaveLockfileError::WriteFile(error));
        }
        // Close the handle before `rename`. Windows `MoveFileEx` over
        // an open source file can fail with sharing-violation; on Unix
        // an early `close` lets the kernel commit dirty buffers before
        // the rename commits the dirent change.
        drop(file);

        return fs::rename(&tmp, target).map_err(|error| {
            // Best-effort cleanup so a failed rename doesn't leak temp
            // files in the virtual store.
            let _ = fs::remove_file(&tmp);
            SaveLockfileError::RenameFile { tmp, target: target.to_path_buf(), error }
        });
    }

    // Ran out of temp-name attempts. Surface the last `AlreadyExists`
    // so the operator can see what happened.
    Err(SaveLockfileError::WriteFile(last_already_exists.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "exhausted temp-path attempts for atomic lockfile write",
        )
    })))
}

#[cfg(test)]
mod tests;
