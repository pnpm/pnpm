use std::{path::Path, sync::Arc};

use pnpm_lockfile::Lockfile;
use pnpm_lockfile_verification::{
    PendingVerificationRecord, VerifyLockfileResolutionsOptions, verify_lockfile_resolutions,
};
use pnpm_resolving_resolver_base::ResolutionVerifier;

use super::{InstallError, LockfileVerificationOverride, errors::map_frozen_lockfile_error};

/// Run the lockfile verification fan-out to completion, blocking the
/// caller on the verdict. Used by the install paths that have no fetch
/// to overlap verification with (fresh resolve, the lockfile-only and
/// up-to-date short-circuits); the frozen materialization path instead
/// runs verification concurrently with the fetch inside
/// [`InstallFrozenLockfile`](pnpm_deps_restorer::InstallFrozenLockfile). A no-op when `verifiers` is empty.
///
/// The verdict is returned unrecorded: see [`PendingVerificationRecord`]
/// for why a caller that goes on to run lifecycle scripts must persist it
/// after them.
async fn verify_lockfile_eagerly<Reporter: pnpm_reporter::Reporter>(
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    lockfile_path: Option<&Path>,
    cache_dir: &Path,
) -> Result<Option<PendingVerificationRecord>, InstallError> {
    if verifiers.is_empty() {
        return Ok(None);
    }
    verify_lockfile_resolutions::<Reporter>(
        lockfile,
        verifiers,
        &VerifyLockfileResolutionsOptions {
            concurrency: None,
            lockfile_path,
            cache_dir: Some(cache_dir),
            replaced: None,
            repairs_lockfile: false,
        },
    )
    .await
    .map_err(InstallError::LockfileVerification)
}

/// Await the verification run elsewhere, or run [`verify_lockfile_eagerly`]
/// when there is none. Either way the verdict comes back unrecorded.
pub(super) async fn await_or_verify_lockfile<Reporter: pnpm_reporter::Reporter>(
    verification_override: Option<LockfileVerificationOverride<'_>>,
    lockfile: &Lockfile,
    verifiers: &[Arc<dyn ResolutionVerifier>],
    lockfile_path: Option<&Path>,
    cache_dir: &Path,
) -> Result<Option<PendingVerificationRecord>, InstallError> {
    match verification_override {
        Some(verification_override) => {
            verification_override.await.map_err(map_frozen_lockfile_error)
        }
        None => {
            verify_lockfile_eagerly::<Reporter>(lockfile, verifiers, lockfile_path, cache_dir).await
        }
    }
}
