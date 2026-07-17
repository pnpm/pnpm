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
/// ahead of the freshly serialized main document, writing
/// `${YAML_DOCUMENT_START}${envDoc}${YAML_DOCUMENT_SEPARATOR}${mainDoc}`.
/// A lockfile with no env document round-trips byte-for-byte (no
/// leading `---`).
///
/// A byte-identical rewrite is skipped, so an up-to-date install leaves the
/// lockfile — and its mtime — untouched, and a symlinked lockfile that nothing
/// changes is never refused (<https://github.com/pnpm/pnpm/issues/13073>). A
/// write that does change bytes refuses a symlinked lockfile.
///
/// The write is atomic: a crash mid-write leaves the previous lockfile intact
/// rather than a truncated one. A missing parent directory is an error, not
/// something to create — the lockfile is written into an existing project.
pub fn save_value_to_path<Document: serde::Serialize>(
    value: &Document,
    path: &Path,
) -> Result<(), SaveLockfileError> {
    let content = serialize_yaml::to_string(value).map_err(SaveLockfileError::SerializeYaml)?;
    let existing = match fs::read_to_string(path) {
        Ok(existing) => {
            Some(existing.strip_prefix('\u{feff}').unwrap_or(&existing).replace("\r\n", "\n"))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(SaveLockfileError::WriteFile(error)),
    };
    let output = match existing.as_deref().and_then(extract_env_document) {
        Some(env) => format!("{YAML_DOCUMENT_START}{env}{YAML_DOCUMENT_SEPARATOR}{content}"),
        None => content,
    };
    if existing.as_deref() == Some(output.as_str()) {
        return Ok(());
    }
    ensure_lockfile_is_not_symlink(path).map_err(SaveLockfileError::WriteFile)?;
    write_atomic(path, output.as_bytes())
}

/// Refuses a symlinked lockfile before a write: the lockfile must be a real file
/// to be written. A writer that resolves the path lands on the link's target, so
/// a repo-planted `pnpm-lock.yaml` redirects the write onto any file the user can
/// write; a writer that renames over the link instead discards a lockfile a build
/// sandbox staged deliberately.
///
/// Reads may follow the link and must not call this. Sandboxes stage
/// `pnpm-lock.yaml` as a symlink (<https://github.com/pnpm/pnpm/issues/13073>),
/// and lockfile content is untrusted however it is reached — a repository can
/// commit whatever content it likes as a plain file.
pub(crate) fn ensure_lockfile_is_not_symlink(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(symlinked_lockfile_error(path)),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(crate) fn symlinked_lockfile_error(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("Refusing to write symlinked lockfile at {}", path.display()),
    )
}

impl Lockfile {
    /// Render lockfile as pnpm-formatted YAML.
    pub fn to_yaml_string(&self) -> Result<String, SaveLockfileError> {
        serialize_yaml::to_string(self).map_err(SaveLockfileError::SerializeYaml)
    }

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
    /// `<virtual_store_dir>/lock.yaml` at end-of-install:
    ///
    /// - When the lockfile is empty ([`Lockfile::is_empty`]) the
    ///   existing file is removed and no new content is written, so an
    ///   empty install doesn't leave stale state behind.
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
            let content = self.to_yaml_string()?;
            write_atomic(&target, content.as_bytes())
        }
    }
}

/// Make the freshly created temp file a faithful stand-in for `target`: flush
/// it to disk so the rename can't publish a directory entry whose data didn't
/// survive a power loss, and carry `target`'s mode across — the rename replaces
/// `target` with this file, so a lockfile the user tightened would otherwise
/// silently revert to the umask default.
fn fill_temp_file(file: &mut fs::File, content: &[u8], target: &Path) -> io::Result<()> {
    file.write_all(content)?;
    file.sync_all()?;
    carry_mode_across(file, target)
}

#[cfg(unix)]
fn carry_mode_across(file: &fs::File, target: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    // `symlink_metadata` so a symlinked target is never followed. Callers
    // refuse a symlinked lockfile before reaching here; a fresh file keeps the
    // umask default, matching a plain create.
    let Ok(metadata) = fs::symlink_metadata(target) else { return Ok(()) };
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    file.set_permissions(fs::Permissions::from_mode(metadata.permissions().mode()))
}

#[cfg(not(unix))]
fn carry_mode_across(_file: &fs::File, _target: &Path) -> io::Result<()> {
    Ok(())
}

/// Write `content` to `target` via a temp file in the same directory
/// followed by `rename`. The rename is atomic on Unix and replaces
/// in-place on Windows, so an observer never sees a torn file, and a
/// crash mid-write leaves the previous `target` intact. A missing parent
/// directory surfaces as an error rather than being created.
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

        if let Err(error) = fill_temp_file(&mut file, content, target) {
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
