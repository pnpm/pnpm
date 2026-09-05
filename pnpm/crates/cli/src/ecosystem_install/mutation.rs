use miette::{IntoDiagnostic, Result, WrapErr};
use std::{collections::BTreeSet, fs, path::PathBuf};

struct FileSnapshot {
    path: PathBuf,
    contents: Option<Vec<u8>>,
}

impl FileSnapshot {
    fn restore(self) -> Result<()> {
        let Self { path, contents } = self;
        let outcome = match contents {
            Some(contents) => pnpm_fs::write_atomic(&path, &contents),
            None => match fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            },
        };
        outcome.into_diagnostic().wrap_err_with(|| format!("restore {}", path.display()))
    }
}

pub(crate) struct MetadataMutation {
    snapshots: Vec<FileSnapshot>,
}

impl MetadataMutation {
    pub(crate) fn capture(paths: impl IntoIterator<Item = PathBuf>) -> Result<Self> {
        let snapshots = paths
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|path| {
                let contents = match fs::read(&path) {
                    Ok(contents) => Some(contents),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => {
                        return Err(error)
                            .into_diagnostic()
                            .wrap_err_with(|| format!("snapshot {}", path.display()));
                    }
                };
                Ok(FileSnapshot { path, contents })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { snapshots })
    }

    pub(crate) fn finish(self, outcome: Result<()>) -> Result<()> {
        let Err(operation_error) = outcome else {
            return Ok(());
        };
        self.restore().map_err(|restore_error| {
            restore_error.wrap_err(format!(
                "restore project metadata after dependency operation failed: {operation_error}"
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
