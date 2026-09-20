use super::metadata_file::MetadataFile;
use miette::{IntoDiagnostic, Result, WrapErr};
use std::{fs, path::PathBuf};

pub(crate) struct MetadataMutation {
    snapshots: Vec<MetadataFile>,
    // Hold the workspace advisory lock from capture through publication or rollback.
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

    fn capture_blocking(
        transaction_key: &std::path::Path,
        mut paths: Vec<PathBuf>,
    ) -> Result<Self> {
        let lock_directory = pnpm_fs::secure_temp_lock_dir("pnpm-metadata-mutation-locks")
            .into_diagnostic()
            .wrap_err("prepare metadata lock directory")?;
        let transaction_key = fs::canonicalize(transaction_key)
            .into_diagnostic()
            .wrap_err_with(|| {
                format!("resolve metadata transaction key {}", transaction_key.display())
            })?;
        let lock_path = lock_directory.join(format!(
            "{}.lock",
            pnpm_crypto_hash::create_hex_hash(&transaction_key.to_string_lossy()),
        ));
        let lock = pnpm_fs::open_secure_lock_file(&lock_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("open metadata transaction lock {}", lock_path.display()))?;
        lock.lock()
            .into_diagnostic()
            .wrap_err_with(|| {
                format!("acquire metadata transaction lock {}", lock_path.display())
            })?;
        paths.sort();
        paths.dedup();
        let snapshots = paths
            .into_iter()
            .map(MetadataFile::capture)
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { snapshots, _lock: lock })
    }

    pub(crate) fn finish(self, outcome: Result<()>) -> Result<()> {
        let Err(operation_error) = outcome else {
            return Ok(());
        };
        self.restore()
            .map_err(|restore_error| {
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

#[cfg(test)]
mod tests;
