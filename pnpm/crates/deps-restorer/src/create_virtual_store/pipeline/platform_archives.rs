//! The store-only pass for `supportedArchitectures`' other platforms: after
//! the install has materialized what it links, the archives its listed
//! platforms keep and the packages those archives depend on are fetched
//! into the store. It runs on every install, warm or cold, and on
//! `pnpm fetch`, so a platform added to the setting is picked up by the next
//! install; an archive already in the store is read from the store index.

use super::{CreateVirtualStore, CreateVirtualStoreError, CreateVirtualStoreStoreContext};
use crate::{
    InstallPackageBySnapshot, KeptArchive, PlatformArchivesInput, PlatformLockfile,
    create_virtual_store::cold::swallow_optional_fetch_failure,
    platform_archives,
    platform_selection::{VariantPlatforms, listed_platforms},
};
use pnpm_lockfile::{PackageKey, PackageMetadata, PlatformSelector, SnapshotEntry};
use pnpm_reporter::Reporter;
use std::collections::HashMap;

impl CreateVirtualStore<'_> {
    /// Fetch into the store the archives `supportedArchitectures` keeps for
    /// its platforms beside the linked ones. Nothing happens unless it lists
    /// a platform other than the one the install links for.
    pub(super) async fn store_platform_archives<Reporter: self::Reporter>(
        &self,
        materialized: &HashMap<PackageKey, SnapshotEntry>,
        store: &CreateVirtualStoreStoreContext<'_>,
    ) -> Result<(), CreateVirtualStoreError> {
        let Some(PlatformLockfile { lockfile, groups }) = self.selection.platform_lockfile else {
            return Ok(());
        };
        let Some(packages) = lockfile.packages.as_ref() else { return Ok(()) };
        let supported = self.selection.supported_architectures;
        let variant_platforms = VariantPlatforms::of(supported);
        let Some(platforms) =
            other_platforms(listed_platforms(supported), &variant_platforms.linked)
        else {
            return Ok(());
        };
        let input = PlatformArchivesInput {
            lockfile,
            roots: self.importers,
            materialized,
            linked: &variant_platforms.linked,
            skipped_peer_edges: &groups.skipped_peer_edges,
            include_optional: self.selection.include_optional,
        };
        let archives = platform_archives(&input, &platforms);
        if archives.is_empty() {
            return Ok(());
        }
        let installer = self.store_installer(store, &variant_platforms);
        self.fetch_kept_archives::<Reporter>(&installer, &archives, packages).await
    }

    /// Fetch each of `archives` into the store, all at once, the client's
    /// per-host limit bounding the downloads. As the cold path does, a fetch
    /// failure of an archive only optional dependencies reach is reported
    /// and skipped.
    async fn fetch_kept_archives<Reporter: self::Reporter>(
        &self,
        installer: &InstallPackageBySnapshot<'_>,
        archives: &[KeptArchive],
        packages: &HashMap<PackageKey, PackageMetadata>,
    ) -> Result<(), CreateVirtualStoreError> {
        let fetched = futures_util::future::join_all(
            archives
                .iter()
                .map(|kept| async {
                    let Some(metadata) = packages.get(&kept.key.without_peer()) else {
                        return Ok(());
                    };
                    installer.store_archive::<Reporter>(kept, metadata).await
                }),
        )
        .await;
        for (kept, result) in archives.iter().zip(fetched) {
            if let Err(error) = result {
                let snapshot =
                    SnapshotEntry { optional: kept.optional, ..SnapshotEntry::default() };
                let requester = self.ctx.requester;
                swallow_optional_fetch_failure::<Reporter, ()>(
                    &kept.key, &snapshot, error, requester,
                )?;
            }
        }
        Ok(())
    }

    fn store_installer<'i>(
        &'i self,
        store: &CreateVirtualStoreStoreContext<'i>,
        variant_platforms: &'i VariantPlatforms,
    ) -> InstallPackageBySnapshot<'i> {
        InstallPackageBySnapshot {
            fetching: crate::SnapshotFetchContext {
                http_client: self.fetching.http_client,
                store_index: store.index,
                store_index_writer: Some(self.fetching.store_index_writer),
                prefetched_cas_paths: None,
                tarball_mem_cache: self.fetching.tarball_mem_cache,
                progress_reported: Some(self.fetching.progress_reported),
                verified_files_cache: store.verified_files_cache,
                custom_fetcher_session: self.fetching.custom_fetcher_session,
            },
            ctx: self.ctx,
            skipped: self.selection.skipped,
            include_optional_dependencies: self.selection.include_optional,
            variant_platforms,
            defer_link: true,
            #[cfg(test)]
            link_concurrency_probe: None,
        }
    }
}

/// `listed`, when one of them is a platform other than `linked`: the walk
/// then keeps the archives of every listed platform, `linked` included.
/// `None` when the install links for each of them already.
fn other_platforms(
    listed: Vec<PlatformSelector>,
    linked: &PlatformSelector,
) -> Option<Vec<PlatformSelector>> {
    listed
        .iter()
        .any(|platform| platform != linked)
        .then_some(listed)
}
