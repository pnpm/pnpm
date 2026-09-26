use crate::{InstallError, install::InstallWithFreshLockfileError};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_lockfile::StalenessReason;

/// Outcome of [`super::check_lockfile_freshness`]. Splits "user
/// configuration is malformed" (always fatal) from "lockfile is stale"
/// (fatal for `--frozen-lockfile`, fall-through to the fresh-resolve
/// path under `preferFrozenLockfile: true`).
#[derive(Debug, Display, Error, Diagnostic)]
pub(crate) enum FreshnessCheckError {
    /// The lockfile has no entry for the root importer.
    #[display(
        r#"Cannot install with "frozen-lockfile" because pnpm-lock.yaml has no `importers["{importer_id}"]` entry. Regenerate the lockfile with `pnpm install --lockfile-only`."#
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_NO_IMPORTER))]
    NoImporter { importer_id: String },

    /// A value in `pnpm.overrides` couldn't be parsed.
    #[diagnostic(transparent)]
    InvalidOverrides(#[error(source)] pnpm_config_parse_overrides::ParseOverridesError),

    /// A configured `patchedDependencies` patch file couldn't be read
    /// or hashed while computing the map to compare against the
    /// lockfile.
    #[diagnostic(transparent)]
    CalcPatchHashes(#[error(source)] pnpm_patching::CalcPatchHashError),

    /// `pnpm-lock.yaml` doesn't match the on-disk `package.json` /
    /// current settings.
    Stale(#[error(not(source))] StalenessReason),
}

impl From<FreshnessCheckError> for InstallError {
    fn from(error: FreshnessCheckError) -> InstallError {
        match error {
            FreshnessCheckError::NoImporter { importer_id } => {
                InstallError::NoImporter { importer_id }
            }
            FreshnessCheckError::InvalidOverrides(inner) => InstallError::InvalidOverrides(inner),
            FreshnessCheckError::CalcPatchHashes(inner) => InstallError::WithFreshLockfile(
                InstallWithFreshLockfileError::CalcPatchHashes(inner),
            ),
            FreshnessCheckError::Stale(StalenessReason::InconsistentPatchHashes) => {
                InstallError::InconsistentPatchHash
            }
            FreshnessCheckError::Stale(StalenessReason::UncheckablePatchHashes) => {
                InstallError::UncheckablePatchHash
            }
            FreshnessCheckError::Stale(reason) => match reason.setting_name() {
                Some(setting) => InstallError::LockfileConfigMismatch { setting },
                None => InstallError::OutdatedLockfile { reason },
            },
        }
    }
}
