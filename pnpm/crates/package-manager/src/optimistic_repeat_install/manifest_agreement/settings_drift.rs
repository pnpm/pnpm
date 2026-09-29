//! Whether the lockfile's recorded settings still match the configuration.

use super::{Lockfile, OptimisticRepeatInstallCheck};
use pnpm_lockfile::StalenessReason;

/// The lockfile's recorded settings must still match the configuration.
pub(super) fn check_settings_match_lockfile(
    check: &OptimisticRepeatInstallCheck<'_>,
    wanted: &Lockfile,
    parsed_overrides: Option<&[pnpm_config_parse_overrides::VersionOverride]>,
    dedupe_peers: bool,
) -> Result<(), &'static str> {
    if let Err(error) = crate::install::check_lockfile_settings_drift(
        wanted,
        check.config,
        check.catalogs,
        crate::install::CheckLockfileSettingsDriftOptions {
            parsed_overrides,
            // `pnpmfileChecksum` is not compared here: every caller already
            // compared the pnpmfile list and trusted their contents by their
            // mtimes (`pnpmfiles_drift`). The move proof refuses active
            // pnpmfiles because their mtimes cannot prove content. Computing
            // the checksum would cost a Node worker on the path that exists to avoid
            // starting one.
            pnpmfile_checksum: pnpm_lockfile::PnpmfileChecksumCheck::Skip,
            dedupe_peers,
        },
    ) {
        tracing::debug!(target: "pacquet::install", %error, "repeat-install content check: lockfile settings drift");
        return Err(settings_drift_reason(&error));
    }
    Ok(())
}

/// The reason a failed lockfile settings check reports. The patch-hash
/// checks compare the lockfile against itself rather than against a setting,
/// so they are named apart.
fn settings_drift_reason(error: &crate::install::FreshnessCheckError) -> &'static str {
    match error {
        crate::install::FreshnessCheckError::Stale(StalenessReason::InconsistentPatchHashes) => {
            r#"the lockfile has patch hashes that disagree with its own "patchedDependencies""#
        }
        crate::install::FreshnessCheckError::Stale(StalenessReason::UncheckablePatchHashes) => {
            "the lockfile cannot be checked for stale patch hashes"
        }
        _ => "a lockfile setting drifted from the current configuration",
    }
}
