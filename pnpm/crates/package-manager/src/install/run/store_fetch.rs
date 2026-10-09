//! The store fetch of a run that writes no `node_modules`. See
//! [`RunMode::fetches_into_store`].

use super::{InstallError, InstallOwned, InstallView, Lockfiles, RunMode};
use pnpm_reporter::Reporter;
use std::sync::Arc;

/// The parts of the run the fetch reads. Not the run itself: a borrow of
/// the whole run across the await would need its boxed verification future
/// to be `Sync`.
#[derive(Clone, Copy)]
pub(super) struct StoreFetchRun<'a> {
    pub(super) mode: &'a RunMode,
    pub(super) install: InstallView<'a>,
    pub(super) owned: &'a InstallOwned,
    pub(super) requester: &'a str,
    pub(super) prefetch_downloads: Option<&'a Arc<crate::PrefetchDownloads>>,
}

/// The frozen half: the wanted lockfile is the resolution, so its packages
/// are fetched from it.
pub(super) async fn fetch_wanted_lockfile<Reporter: self::Reporter + 'static>(
    run: StoreFetchRun<'_>,
    lockfiles: &Lockfiles<'_>,
) -> Result<(), InstallError> {
    if !run.mode.fetches_into_store(run.install.execution) {
        return Ok(());
    }
    let lockfile = lockfiles.wanted.get().expect("frozen dispatch verified lockfile is present");
    crate::store_fetch::fetch_lockfile_into_store::<Reporter>(
        crate::store_fetch::StoreFetchInputs {
            lockfile,
            config: run.install.context.config,
            http_client: &run.owned.http_client_arc,
            mem_cache: &run.owned.tarball_mem_cache,
            auth_override: run.owned.resolution.auth_override.as_ref(),
            requester: run.requester,
            supported_architectures: run.owned.projects.supported_architectures.as_ref(),
        },
    )
    .await
}

/// The fresh half: the resolver prefetched each tarball as it resolved it,
/// and a run that materializes nothing waits for those downloads itself.
pub(super) async fn wait_for_prefetched_tarballs(
    run: StoreFetchRun<'_>,
) -> Result<(), InstallError> {
    let Some(downloads) = run.prefetch_downloads else {
        return Ok(());
    };
    crate::store_fetch::wait_for_prefetched_downloads(downloads).await
}
