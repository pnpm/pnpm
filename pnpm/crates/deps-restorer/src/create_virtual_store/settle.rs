//! Await the warm-cache prefetch and run the checks the plan pass
//! decides: the deferred CAFS files checks of the rows this install
//! imports, and the cached git-prepare policy.

use super::{
    CreateVirtualStore, CreateVirtualStoreError, PrefetchTask, snapshot_plan,
    warm::enforce_cached_git_prepare_policy,
};
use pnpm_lockfile::{PackageKey, PackageMetadata};
use pnpm_store_dir::{SharedVerifiedFilesCache, StoreDir};
use pnpm_tarball::PrefetchResult;
use std::collections::HashMap;

impl CreateVirtualStore<'_> {
    /// A joined-task failure degrades to an empty result: every lookup
    /// misses and the snapshots fall through to their per-snapshot
    /// path, the same shape as a store with no index.
    pub(super) async fn settle_prefetch(
        &self,
        task: PrefetchTask,
        verified_files_cache: &SharedVerifiedFilesCache,
        packages: &HashMap<PackageKey, PackageMetadata>,
        plan: &mut snapshot_plan::SnapshotPlan<'_>,
    ) -> Result<PrefetchResult, CreateVirtualStoreError> {
        let prefetched = match task {
            PrefetchTask::Settled(prefetched) => *prefetched,
            PrefetchTask::Running(task) => task.await.unwrap_or_else(|error| {
                tracing::warn!(
                    target: "pacquet::install",
                    ?error,
                    "warm-cache prefetch task failed; treating every lookup as a miss",
                );
                PrefetchResult::default()
            }),
        };
        let prefetched = self.verify_imported_rows(prefetched, verified_files_cache, plan).await;
        enforce_cached_git_prepare_policy(
            &mut plan.survivors,
            packages,
            &prefetched,
            self.ctx.allow_build_policy,
            self.ctx.config.ignore_scripts,
            plan.has_git_hosted_survivor,
        )?;
        Ok(prefetched)
    }

    /// Run the files checks the prefetch deferred for the rows whose
    /// files this install may import: every survivor, and every skipped
    /// snapshot whose row carries a side-effects overlay, which the
    /// build phase's cache hit materializes into the slot. A row whose
    /// CAFS files have gone missing is dropped and re-fetched. The other
    /// skipped snapshots keep their rows unchecked: nothing imports
    /// their files, and their manifests and `requiresBuild` flags come
    /// from the row itself.
    async fn verify_imported_rows(
        &self,
        mut prefetched: PrefetchResult,
        verified_files_cache: &SharedVerifiedFilesCache,
        plan: &snapshot_plan::SnapshotPlan<'_>,
    ) -> PrefetchResult {
        if prefetched.pending_checks.is_empty() {
            return prefetched;
        }
        let imported_keys = imported_cache_keys(plan, &prefetched);
        let store_dir: &'static StoreDir = &self.ctx.config.store_dir;
        let verified_files_cache = SharedVerifiedFilesCache::clone(verified_files_cache);
        tokio::task::spawn_blocking(move || {
            let verify_start = std::time::Instant::now();
            let failed = prefetched.verify_rows(
                imported_keys.iter().map(String::as_str),
                store_dir,
                &verified_files_cache,
            );
            tracing::debug!(
                target: "pacquet::download",
                rows = imported_keys.len(),
                failed,
                verify_ms = verify_start.elapsed().as_millis() as u64,
                "store rows of the imported snapshots verified",
            );
            prefetched
        })
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(
                target: "pacquet::install",
                ?error,
                "store-row verification task failed; treating every lookup as a miss",
            );
            PrefetchResult::default()
        })
    }
}

fn imported_cache_keys(
    plan: &snapshot_plan::SnapshotPlan<'_>,
    prefetched: &PrefetchResult,
) -> Vec<String> {
    plan.survivors
        .iter()
        .filter_map(|(_, _, cache_key)| cache_key.clone())
        .chain(
            plan.skipped_entries
                .iter()
                .filter_map(|(_, _, cache_key)| cache_key.clone())
                .filter(|cache_key| prefetched.side_effects_maps.contains_key(cache_key)),
        )
        .collect()
}
