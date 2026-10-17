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

    #[diagnostic(transparent)]
    InvalidCatalog(#[error(source)] pnpm_catalogs_resolver::CatalogResolutionError),

    /// A configured `patchedDependencies` patch file couldn't be read
    /// or hashed while computing the map to compare against the
    /// lockfile.
    #[diagnostic(transparent)]
    CalcPatchHashes(#[error(source)] pnpm_patching::CalcPatchHashError),

    /// A `packageExtensions` selector could not be parsed while applying the
    /// extensions to an injected dependency's manifest.
    #[diagnostic(transparent)]
    InvalidPackageExtensionSelector(
        #[error(source)] crate::package_extender::InvalidPackageExtensionSelector,
    ),

    /// The pnpmfile's `readPackage` failed on an injected dependency's
    /// manifest.
    ReadPackageHook(#[error(not(source))] pnpm_hooks::HookError),

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
            FreshnessCheckError::InvalidCatalog(inner) => InstallError::CatalogResolution(inner),
            FreshnessCheckError::CalcPatchHashes(inner) => InstallError::WithFreshLockfile(
                InstallWithFreshLockfileError::CalcPatchHashes(inner),
            ),
            FreshnessCheckError::InvalidPackageExtensionSelector(inner) => {
                InstallError::InvalidPackageExtensionSelector(inner)
            }
            FreshnessCheckError::ReadPackageHook(inner) => InstallError::from(inner),
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
