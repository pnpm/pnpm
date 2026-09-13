use super::{ArtifactCleanupError, Diagnostic, Display, Error, PathBuf};

#[derive(Debug, Display, Error, Diagnostic)]
pub(super) enum GlobalActivationError {
    #[display(
        "Cannot replace global bin slot at {}: expected a regular file or symbolic link",
        path.display()
    )]
    #[diagnostic(code(ERR_PNPM_GLOBAL_BIN_UNSUPPORTED_TYPE))]
    UnsupportedType { path: PathBuf },

    #[display(
        "Failed to restore global bins after activation failed. Recovery files remain at {}; the fresh install remains at {}. Rollback error: {rollback_error}",
        backup_dir.display(),
        install_dir.display()
    )]
    #[diagnostic(code(ERR_PNPM_GLOBAL_BIN_ROLLBACK_FAILED))]
    RollbackFailed {
        backup_dir: PathBuf,
        install_dir: PathBuf,
        rollback_error: String,
        #[error(source)]
        #[diagnostic_source]
        activation_error: Box<dyn Diagnostic + Send + Sync>,
    },

    #[display(
        "Failed to restore global bins after replacement failed. Recovery files remain at {}. Rollback error: {rollback_error}",
        backup_dir.display()
    )]
    #[diagnostic(code(ERR_PNPM_GLOBAL_BIN_ROLLBACK_FAILED))]
    BinReplacementRollbackFailed {
        backup_dir: PathBuf,
        rollback_error: String,
        #[error(source)]
        #[diagnostic_source]
        replacement_error: Box<dyn Diagnostic + Send + Sync>,
    },

    #[display("Failed to restore all global bin slots")]
    BinSlotRestorationFailed {
        #[error(not(source))]
        #[related]
        failures: Vec<ArtifactCleanupError>,
    },

    #[display("Failed to clean up after global bin activation failed.{remaining_artifacts}")]
    RollbackCleanupFailed {
        remaining_artifacts: String,
        #[error(not(source))]
        #[related]
        cleanup_reports: Vec<ArtifactCleanupError>,
        #[error(source)]
        #[diagnostic_source]
        activation_error: Box<dyn Diagnostic + Send + Sync>,
    },
}
