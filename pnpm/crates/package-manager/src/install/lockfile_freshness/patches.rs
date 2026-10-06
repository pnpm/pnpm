//! The freshness check of a rebuild, which builds the locked packages
//! without resolving or linking them: only the patches decide what it
//! builds, so they are all that has to match the configuration.

use super::{
    super::{Config, Lockfile},
    FreshnessCheckError, FreshnessScope, LockfileFreshnessInputs,
};
use crate::RebuildOptions;

impl LockfileFreshnessInputs<'_, '_> {
    /// Narrows the check to the lockfile's patches when `rebuild` asks
    /// for it. See [`RebuildOptions::check_lockfile_patches_only`].
    pub(crate) fn for_rebuild(self, rebuild: Option<&RebuildOptions>) -> Self {
        let patches_only = rebuild.is_some_and(|rebuild| rebuild.check_lockfile_patches_only);
        LockfileFreshnessInputs { scope: FreshnessScope { patches_only, ..self.scope }, ..self }
    }
}

/// The lockfile's `patchedDependencies` must match the configured patch
/// hashes, and its `(patch_hash=...)` depPath suffixes must match them.
pub(super) fn check_lockfile_patches(
    lockfile: &Lockfile,
    config: &Config,
) -> Result<(), FreshnessCheckError> {
    let patched_dependency_hashes =
        config.patched_dependency_hashes().map_err(FreshnessCheckError::CalcPatchHashes)?;
    pnpm_lockfile::check_lockfile_patches(lockfile, patched_dependency_hashes.as_ref())
        .map_err(FreshnessCheckError::Stale)
}
