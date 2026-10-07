//! The settings part of the freshness gate. A rebuild may narrow it to
//! the patches: it builds the locked packages without resolving them, so
//! of the settings only the patches decide what it builds.

use super::{
    super::{Config, Lockfile},
    CheckLockfileSettingsDriftOptions, FreshnessCheckError, FreshnessScope,
    LockfileFreshnessInputs, check_lockfile_settings_drift, parse_config_overrides,
    parse_overrides,
};
use crate::RebuildOptions;
use pnpm_config_parse_overrides::VersionOverride;

impl LockfileFreshnessInputs<'_, '_> {
    /// Narrows the settings check to the lockfile's patches when `rebuild`
    /// asks for it. See [`RebuildOptions::check_lockfile_patches_only`].
    pub(crate) fn for_rebuild(self, rebuild: Option<&RebuildOptions>) -> Self {
        let patches_only = rebuild.is_some_and(|rebuild| rebuild.check_lockfile_patches_only);
        LockfileFreshnessInputs { scope: FreshnessScope { patches_only, ..self.scope }, ..self }
    }
}

/// The settings the lockfile records must match the current ones: all of
/// them, or only the patches when [`FreshnessScope::patches_only`] is set.
///
/// Returns the overrides the manifests are checked against the lockfile
/// with: the current ones, or for a patches-only check the ones the
/// lockfile records, as the overrides are allowed to have drifted.
pub(super) async fn check_settings(
    lockfile: &Lockfile,
    inputs: &LockfileFreshnessInputs<'_, '_>,
) -> Result<Option<Vec<VersionOverride>>, FreshnessCheckError> {
    if inputs.scope.patches_only {
        check_lockfile_patches(lockfile, inputs.config)?;
        return parse_overrides(lockfile.overrides.as_ref(), inputs.catalogs);
    }
    let parsed_overrides = parse_config_overrides(inputs.config, inputs.catalogs)?;
    let pnpmfile_checksum = pnpm_hooks::current_pnpmfile_checksum(
        inputs.pnpmfile_hook,
        lockfile.pnpmfile_checksum.as_deref(),
    )
    .await;
    check_lockfile_settings_drift(
        lockfile,
        inputs.config,
        inputs.catalogs,
        CheckLockfileSettingsDriftOptions {
            parsed_overrides: parsed_overrides.as_deref(),
            pnpmfile_checksum: super::super::pnpmfile_checksum_check(
                inputs,
                pnpmfile_checksum.as_deref(),
            ),
            dedupe_peers: inputs.config.dedupe_peers,
        },
    )?;
    Ok(parsed_overrides)
}

/// The lockfile's `patchedDependencies` must match the configured patch
/// hashes, and its `(patch_hash=...)` depPath suffixes must match them.
fn check_lockfile_patches(lockfile: &Lockfile, config: &Config) -> Result<(), FreshnessCheckError> {
    let patched_dependency_hashes =
        config.patched_dependency_hashes().map_err(FreshnessCheckError::CalcPatchHashes)?;
    pnpm_lockfile::check_lockfile_patches(lockfile, patched_dependency_hashes.as_ref())
        .map_err(FreshnessCheckError::Stale)
}
