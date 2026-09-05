use super::metadata_file::MetadataFile;
use miette::{IntoDiagnostic, Result, WrapErr};
use std::{collections::BTreeSet, fs, path::PathBuf};

pub(crate) struct MetadataMutation {
    snapshots: Vec<MetadataFile>,
    // Every mixed add using this store and Cargo workspace takes the same
    // advisory lock from before capture through either commit or rollback.
    // Keeping it here makes the transaction lifetime structural.
    _lock: fs::File,
}

impl MetadataMutation {
    pub(crate) async fn capture(
        lock_directory: PathBuf,
        transaction_key: PathBuf,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Self> {
        let paths = paths.into_iter().collect::<Vec<_>>();
        tokio::task::spawn_blocking(move || {
            Self::capture_blocking(&lock_directory, &transaction_key, paths)
        })
        .await
        .into_diagnostic()
        .wrap_err("join metadata snapshot task")?
    }

    fn capture_blocking(
        lock_directory: &std::path::Path,
        transaction_key: &std::path::Path,
        paths: Vec<PathBuf>,
    ) -> Result<Self> {
        fs::create_dir_all(lock_directory).into_diagnostic().wrap_err_with(|| {
            format!("create metadata lock directory {}", lock_directory.display())
        })?;
        let transaction_key =
            fs::canonicalize(transaction_key).into_diagnostic().wrap_err_with(|| {
                format!("resolve metadata transaction key {}", transaction_key.display())
            })?;
        let lock_path = lock_directory.join(format!(
            "{}.lock",
            pnpm_crypto_hash::create_hex_hash(&transaction_key.to_string_lossy()),
        ));
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("open metadata transaction lock {}", lock_path.display()))?;
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

#[cfg(test)]
mod tests;
