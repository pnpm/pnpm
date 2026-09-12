use super::{InstallFrozenLockfileError, LockfileVerificationOverride};
use crate::{CreateVirtualStore, CreateVirtualStoreOutput};
use pnpm_lockfile::Lockfile;
use pnpm_lockfile_verification::{VerifyLockfileResolutionsOptions, verify_lockfile_resolutions};
use pnpm_reporter::Reporter;
use pnpm_resolving_resolver_base::ResolutionVerifier;
use std::{path::Path, sync::Arc};

/// The lockfile verification that runs alongside the fetch.
///
/// `precomputed` is a verdict the caller already has in flight; when it
/// is set the verifiers are not consulted. An empty `verifiers` with no
/// `precomputed` verdict means `trustLockfile` — verification is a
/// no-op.
pub(super) struct ConcurrentVerification<'a> {
    pub(super) lockfile: &'a Lockfile,
    pub(super) verifiers: &'a [Arc<dyn ResolutionVerifier>],
    pub(super) precomputed: Option<LockfileVerificationOverride<'a>>,
    pub(super) lockfile_path: Option<&'a Path>,
    pub(super) cache_dir: &'a Path,
}
/// Materialize the virtual store while verifying the lockfile, and
/// return the fetch's output once the lockfile is known trusted.
///
/// The two run concurrently so the verifiers' per-entry registry round
/// trips overlap the downloads. A rejected lockfile aborts the fetch in
/// flight, and a verdict is always reached before this returns, so no
/// dependency lifecycle script can run on an unverified lockfile.
///
/// The verification verdict takes precedence over a fetch error: a plain
/// `try_join!` would surface whichever error landed first, letting an
/// unrelated fetch failure mask a rejected lockfile. So a fetch failure
/// waits for the verdict and only surfaces once the lockfile is trusted.
pub(super) async fn fetch_verified<Reporter: self::Reporter>(
    create_virtual_store: CreateVirtualStore<'_>,
    verification: ConcurrentVerification<'_>,
) -> Result<CreateVirtualStoreOutput, InstallFrozenLockfileError> {
    let ConcurrentVerification { lockfile, verifiers, precomputed, lockfile_path, cache_dir } =
        verification;
    let verify = async {
        if let Some(precomputed) = precomputed {
            return precomputed.await;
        }
        if verifiers.is_empty() {
            return Ok(());
        }
        verify_lockfile_resolutions::<Reporter>(
            lockfile,
            verifiers,
            &VerifyLockfileResolutionsOptions {
                concurrency: None,
                lockfile_path,
                cache_dir: Some(cache_dir),
            },
        )
        .await
        .map_err(InstallFrozenLockfileError::LockfileVerification)
    };
    let fetch = async {
        create_virtual_store
            .run::<Reporter>()
            .await
            .map_err(InstallFrozenLockfileError::CreateVirtualStore)
    };

    let mut verify = std::pin::pin!(verify);
    let mut fetch = std::pin::pin!(fetch);
    tokio::select! {
        verdict = &mut verify => {
            verdict?;
            fetch.await
        }
        output = &mut fetch => {
            verify.await?;
            output
        }
    }
}
/// Load custom fetchers from the install's pnpmfiles, if any.
/// Returns `Ok(None)` when no pnpmfile exists or it exports no
/// fetchers, so the install path can skip the IPC overhead entirely.
/// A pnpmfile that fails to load or evaluate aborts the install, like
/// the custom-resolver load on the fresh-lockfile path.
pub(super) async fn load_custom_fetcher_session(
    hook: Option<&Arc<dyn pnpm_hooks::PnpmfileHooks>>,
) -> Result<Option<Arc<crate::CustomFetcherSession>>, InstallFrozenLockfileError> {
    let Some(hook) = hook else { return Ok(None) };
    let fetchers = hook.get_custom_fetchers().await.map_err(|err| {
        tracing::error!(
            target: "pacquet::install",
            "Failed to get custom fetchers from pnpmfile: {err}",
        );
        InstallFrozenLockfileError::CustomFetcherHook(err)
    })?;
    if fetchers.is_empty() {
        return Ok(None);
    }
    Ok(Some(Arc::new(crate::CustomFetcherSession::new(fetchers))))
}
