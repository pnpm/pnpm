use super::metadata_file::MetadataFile;
use miette::{IntoDiagnostic, Result, WrapErr};
use std::{collections::BTreeSet, fs, path::PathBuf};

pub(crate) struct MetadataMutation {
    snapshots: Vec<MetadataFile>,
    // Every mixed add targeting this Cargo workspace takes the same advisory
    // lock from before capture through either commit or rollback.
    // Keeping it here makes the transaction lifetime structural.
    _lock: fs::File,
}

impl MetadataMutation {
    pub(crate) async fn capture(
        transaction_key: PathBuf,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Self> {
        let paths = paths.into_iter().collect::<Vec<_>>();
        tokio::task::spawn_blocking(move || Self::capture_blocking(&transaction_key, paths))
            .await
            .into_diagnostic()
            .wrap_err("join metadata snapshot task")?
    }

    fn capture_blocking(transaction_key: &std::path::Path, paths: Vec<PathBuf>) -> Result<Self> {
        let lock_directory = metadata_lock_directory();
        prepare_metadata_lock_directory(&lock_directory)?;
        let transaction_key =
            fs::canonicalize(transaction_key).into_diagnostic().wrap_err_with(|| {
                format!("resolve metadata transaction key {}", transaction_key.display())
            })?;
        let lock_path = lock_directory.join(format!(
            "{}.lock",
            pnpm_crypto_hash::create_hex_hash(&transaction_key.to_string_lossy()),
        ));
        let lock = open_metadata_lock(&lock_path)?;
        lock.lock().into_diagnostic().wrap_err_with(|| {
            format!("acquire metadata transaction lock {}", lock_path.display())
        })?;
        let snapshots = paths
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(MetadataFile::capture)
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { snapshots, _lock: lock })
    }

    pub(crate) fn finish(self, outcome: Result<()>) -> Result<()> {
        let Err(operation_error) = outcome else {
            return Ok(());
        };
        self.restore().map_err(|restore_error| {
            restore_error.wrap_err(format!(
                "restore project metadata after dependency operation failed: {operation_error}",
            ))
        })?;
        Err(operation_error)
    }

    fn restore(self) -> Result<()> {
        let mut first_error = None;
        for snapshot in self.snapshots.into_iter().rev() {
            if let Err(error) = snapshot.restore()
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

fn metadata_lock_directory() -> PathBuf {
    let mut directory = std::env::temp_dir();
    #[cfg(unix)]
    directory.push(format!(
        "pnpm-metadata-mutation-locks-{}",
        // SAFETY: `geteuid` has no preconditions and does not mutate memory.
        unsafe { libc::geteuid() },
    ));
    #[cfg(not(unix))]
    directory.push("pnpm-metadata-mutation-locks");
    directory
}

fn prepare_metadata_lock_directory(directory: &std::path::Path) -> Result<()> {
    fs::create_dir_all(directory)
        .into_diagnostic()
        .wrap_err_with(|| format!("create metadata lock directory {}", directory.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        let metadata = fs::symlink_metadata(directory)
            .into_diagnostic()
            .wrap_err_with(|| format!("inspect metadata lock directory {}", directory.display()))?;
        // SAFETY: `geteuid` has no preconditions and does not mutate memory.
        let effective_user = unsafe { libc::geteuid() };
        if !metadata.is_dir() || metadata.uid() != effective_user {
            let directory = directory.display();
            return Err(miette::miette!(
                "metadata lock directory must be a real directory owned by the current user: {}",
                directory,
            ));
        }
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .into_diagnostic()
            .wrap_err_with(|| format!("secure metadata lock directory {}", directory.display()))?;
    }
    Ok(())
}

fn open_metadata_lock(path: &std::path::Path) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;

        options.mode(0o600).custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    let lock = options
        .open(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("open metadata transaction lock {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;

        let metadata = lock
            .metadata()
            .into_diagnostic()
            .wrap_err_with(|| format!("inspect metadata transaction lock {}", path.display()))?;
        // SAFETY: `geteuid` has no preconditions and does not mutate memory.
        let effective_user = unsafe { libc::geteuid() };
        if !metadata.is_file() || metadata.uid() != effective_user || metadata.nlink() != 1 {
            let path = path.display();
            return Err(miette::miette!(
                "metadata transaction lock must be a regular file owned by the current user: {}",
                path,
            ));
        }
    }
    Ok(lock)
}

#[cfg(test)]
mod tests;
