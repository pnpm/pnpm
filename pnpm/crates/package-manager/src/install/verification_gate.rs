use std::{path::Path, sync::Arc};

use pnpm_lockfile::{Lockfile, PkgName};
use pnpm_lockfile_verification::{
    ReplacedEntries, VerifyLockfileResolutionsOptions, verify_lockfile_resolutions,
};
use pnpm_resolving_resolver_base::ResolutionVerifier;

/// The pre-resolve lockfile-verification fan-out, spawned so its
/// registry round trips overlap the fresh path's resolve and
/// materialization instead of serializing in front of them — the same
/// concurrent-gate contract the frozen path's `select!` provides. The
/// verdict still gates everything sensitive:
/// [`InstallWithFreshLockfile`](crate::install_with_fresh_lockfile::InstallWithFreshLockfile)
/// awaits the gate before bin linking, dependency builds, and the lockfile save.
///
/// Aborts the fan-out on drop so an install that fails before reaching
/// the gate doesn't leave verification requests running in the host
/// process (the napi embedding outlives a failed install).
pub struct LockfileVerificationGate(
    tokio::task::JoinHandle<Result<(), pnpm_lockfile_verification::VerifyError>>,
);

/// Owned form of [`ReplacedEntries`], for the spawned gate.
pub(crate) type IsReplaced = Arc<dyn Fn(&PkgName, &str) -> bool + Send + Sync>;

impl LockfileVerificationGate {
    /// Start the fan-out in the background, or `None` when no verifier
    /// is active (`trustLockfile`).
    pub(super) fn spawn<Reporter: pnpm_reporter::Reporter + Send + 'static>(
        lockfile: Arc<Lockfile>,
        verifiers: &[Arc<dyn ResolutionVerifier>],
        lockfile_path: Option<&Path>,
        cache_dir: &Path,
        replaced: Option<IsReplaced>,
        repairs_lockfile: bool,
    ) -> Option<Self> {
        if verifiers.is_empty() {
            return None;
        }
        let verifiers = verifiers.to_vec();
        let lockfile_path = lockfile_path.map(Path::to_path_buf);
        let cache_dir = cache_dir.to_path_buf();
        Some(Self(tokio::spawn(async move {
            verify_lockfile_resolutions::<Reporter>(
                &lockfile,
                &verifiers,
                &VerifyLockfileResolutionsOptions {
                    concurrency: None,
                    lockfile_path: lockfile_path.as_deref(),
                    cache_dir: Some(&cache_dir),
                    replaced: replaced.as_deref().map(ReplacedEntries),
                    repairs_lockfile,
                },
            )
            .await
        })))
    }

    /// Block on the verdict.
    pub(crate) async fn wait(mut self) -> Result<(), pnpm_lockfile_verification::VerifyError> {
        (&mut self.0).await.expect(
            "the lockfile verification task is only aborted by dropping the gate unawaited",
        )
    }
}

impl Drop for LockfileVerificationGate {
    fn drop(&mut self) {
        self.0.abort();
    }
}
