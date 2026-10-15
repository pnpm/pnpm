//! The store handles a fresh resolution opens, and the verification of the
//! wanted lockfile's rows a run that fetches into the store starts with them.

use super::{FreshInputs, OwnedInputs, resolver_setup};

/// Open the store handles, and start verifying the wanted lockfile's rows
/// for a run that fetches into the store.
pub(super) async fn open_stores(
    install: FreshInputs<'_>,
    owned: &OwnedInputs,
    progress_reported: Option<pnpm_tarball::SharedReportedProgressKeys>,
    store_dir: &'static pnpm_store_dir::StoreDir,
) -> resolver_setup::StoreIndexHandles {
    let stores = resolver_setup::open_store_index_handles(
        install.drivers.config,
        store_dir,
        resolver_setup::StoreCaches::new(progress_reported, owned.fetching.fetch_caches.as_ref()),
    )
    .await;
    start_store_verification(install, owned, &stores);
    stores
}

/// Verify the wanted lockfile's rows while the resolution runs, for a run
/// that fetches into the store: the packages the resolution reuses from the
/// lockfile never reach the resolver's prefetch, and their verification is
/// what the fetch after the resolution would otherwise spend its time on.
fn start_store_verification(
    install: FreshInputs<'_>,
    owned: &OwnedInputs,
    stores: &resolver_setup::StoreIndexHandles,
) {
    if !install.execution.fetches_into_store() {
        return;
    }
    let (Some(verification), Some(lockfile), Some(index)) = (
        owned.fetching.store_verification.as_ref(),
        install.lockfiles.wanted,
        stores.index.as_ref(),
    ) else {
        return;
    };
    let config = install.drivers.config;
    verification.start(
        crate::tarball_prefetch::store_keys(
            lockfile,
            config,
            install.projects.supported_architectures,
        ),
        index,
        &config.store_dir,
        config.verify_store_integrity,
        &stores.caches.verified_files,
    );
}
