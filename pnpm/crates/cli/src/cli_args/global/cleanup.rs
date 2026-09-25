use super::{
    ArtifactCleanupError, CmdShimHost, Diagnostic, Display, Error, FsGlobalRemoval, Path, fs, io,
};

/// Discard the half-built install directory when a step of the global
/// install fails. It holds only this group's install, so leaving it
/// behind would accumulate across failed runs. A discard that fails is
/// reported alongside the step's own error, naming the directory it left
/// behind.
pub(super) fn discard_install_dir_on_error<Output, Failure>(
    install_dir: &Path,
    result: Result<Output, Failure>,
) -> miette::Result<Output>
where
    Failure: Into<miette::Report>,
{
    discard_install_dir_on_error_with_fs::<CmdShimHost, _, _>(install_dir, result)
}

pub(super) fn discard_install_dir_on_error_with_fs<Sys, Output, Failure>(
    install_dir: &Path,
    result: Result<Output, Failure>,
) -> miette::Result<Output>
where
    Sys: FsRemoveDirAll,
    Failure: Into<miette::Report>,
{
    let install_error = match result {
        Ok(output) => return Ok(output),
        Err(error) => error.into(),
    };
    Err(match Sys::remove_dir_all(install_dir) {
        Ok(()) => install_error,
        Err(error) if error.kind() == io::ErrorKind::NotFound => install_error,
        Err(source) => GlobalInstallPreparationCleanupError {
            cleanup_reports: vec![ArtifactCleanupError {
                context: format!(
                    "remove fresh global install directory at {}",
                    install_dir.display(),
                ),
                source,
            }],
            install_error: install_error.into(),
        }
        .into(),
    })
}

pub(super) trait FsRemoveDirAll {
    fn remove_dir_all(path: &Path) -> io::Result<()>;
}

impl FsRemoveDirAll for CmdShimHost {
    fn remove_dir_all(path: &Path) -> io::Result<()> {
        fs::remove_dir_all(path)
    }
}

#[derive(Debug, Display, Error, Diagnostic)]
#[display("Failed to clean up after global install failed before activation")]
struct GlobalInstallPreparationCleanupError {
    #[error(not(source))]
    #[related]
    cleanup_reports: Vec<ArtifactCleanupError>,
    #[error(source)]
    #[diagnostic_source]
    install_error: Box<dyn Diagnostic + Send + Sync>,
}

impl FsGlobalRemoval for CmdShimHost {}
