//! The cold half of the pipeline: downloading the snapshots the store
//! lacks, with the remote side-effects resolution alongside.

use super::{
    CasIndexes, ColdInputs, ConcurrentCheck, CreateVirtualStore, CreateVirtualStoreError,
    partition, run_checked,
};
use crate::{
    InstallPackageBySnapshot,
    create_virtual_store::{
        RemoteSideEffectsQuarantineBySnapshot,
        cold::{ColdBatch, ColdBatchState, run_cold_batch, unlink_fetch_failed_children},
    },
    install_package_by_snapshot::runtime_platform_selector,
    shared_side_effects::{
        PreparedSharedSideEffects, ResolvedSharedSideEffects, resolve_prepared_side_effects,
    },
};
use futures_util::FutureExt;
use pnpm_lockfile::{PackageKey, PlatformSelector};
use pnpm_reporter::Reporter;
use std::collections::HashSet;

impl<'a> CreateVirtualStore<'a> {
    /// Snapshots that did not prefetch fall through to the tokio +
    /// download path. An optional snapshot whose fetch fails is dropped
    /// rather than aborting the install; the returned set holds those
    /// keys for the caller to fold into its [`crate::SkippedSnapshots`],
    /// so downstream walkers (`build_graph`, `link_bins`, hoist) treat
    /// the snapshot as absent. Under the hoisted linker no slot is
    /// written and each download's CAS index is the only output, folded
    /// into [`CasIndexes::by_pkg_id`]; the isolated linker's slot import
    /// has already happened by the time the download future returns.
    pub(super) async fn download_and_apply_side_effects<Reporter: self::Reporter>(
        &self,
        inputs: ColdInputs<'_, 'a>,
        (partition, indexes): (&mut partition::Partition<'a>, &mut CasIndexes),
        check: &mut Option<ConcurrentCheck<'_>>,
    ) -> Result<HashSet<PackageKey>, CreateVirtualStoreError> {
        let side_effects_ctx = self.side_effects_context(&inputs);
        if !crate::shared_side_effects::cold_shared_packages_have_known_requires_build(
            &partition.cold,
            inputs.links.shared_packages.as_ref(),
            &partition.requires_build_by_snapshot,
        ) {
            let fetch_failed =
                self.download_cold::<Reporter>(inputs, partition, indexes, check).await?;
            let apply = partition.apply_side_effects(side_effects_ctx, &indexes.shared_base);
            run_checked(check, apply.map(Ok)).await?;
            return Ok(fetch_failed);
        }
        let prepare = partition.prepare_side_effects(side_effects_ctx, &indexes.shared_base);
        let prepared = run_checked(check, prepare.map(Ok)).await?;
        let mut resolve_task =
            spawn_side_effects_resolution(prepared, &partition.side_effects.quarantine);
        let fetch_failed = self
            .download_cold::<Reporter>(inputs, partition, indexes, check)
            .await
            .inspect_err(|_| resolve_task.abort())?;
        let resolved = run_checked(check, (&mut resolve_task).map(joined_resolution))
            .await
            .inspect_err(|_| resolve_task.abort())?;
        let finish =
            partition.finish_side_effects(resolved, side_effects_ctx, &indexes.shared_base);
        run_checked(check, finish.map(Ok)).await?;
        Ok(fetch_failed)
    }

    fn side_effects_context<'i>(
        &'i self,
        inputs: &ColdInputs<'_, 'a>,
    ) -> partition::PartitionSideEffectsContext<'i> {
        partition::PartitionSideEffectsContext {
            config: self.ctx.config,
            wanted: inputs.wanted,
            allow_build_policy: self.ctx.allow_build_policy,
            store_index_writer: self.fetching.store_index_writer,
            importers: self.importers,
        }
    }

    async fn download_cold<Reporter: self::Reporter>(
        &self,
        inputs: ColdInputs<'_, 'a>,
        partition: &mut partition::Partition<'a>,
        indexes: &mut CasIndexes,
        check: &mut Option<ConcurrentCheck<'_>>,
    ) -> Result<HashSet<PackageKey>, CreateVirtualStoreError> {
        let runtime_platform_selector =
            runtime_platform_selector(self.selection.supported_architectures);
        let mut fetch_failed = HashSet::new();
        let mut cold_cas_paths = Vec::new();
        run_cold_batch::<Reporter>(
            ColdBatch {
                cold: &partition.cold,
                installer: self.cold_installer(&inputs, &runtime_platform_selector),
                reuse: self.slot_reuse(inputs.wanted.packages),
                marker_source: inputs.marker_source,
                removed_aliases_by_key: &inputs.links.removed_aliases_by_key,
                link_template: &inputs.links.template,
                shared_packages: inputs.links.shared_packages.as_ref(),
            },
            &mut ColdBatchState {
                fetch_failed: &mut fetch_failed,
                package_manifests: &mut partition.package_manifests,
                package_bins: &mut partition.package_bins,
                requires_build_by_snapshot: &mut partition.requires_build_by_snapshot,
                shared_base_cas_paths: &mut indexes.shared_base,
            },
            &mut cold_cas_paths,
            check,
        )
        .await?;
        indexes.add_cold(cold_cas_paths);
        unlink_fetch_failed_children(self.ctx, inputs.wanted.snapshots, &fetch_failed)?;
        Ok(fetch_failed)
    }

    // Defer slot links to the cold link thread, outside the cooperative download tasks.
    fn cold_installer<'i>(
        &'i self,
        inputs: &'i ColdInputs<'_, 'a>,
        runtime_platform_selector: &'i PlatformSelector,
    ) -> InstallPackageBySnapshot<'i> {
        InstallPackageBySnapshot {
            fetching: crate::SnapshotFetchContext {
                http_client: self.fetching.http_client,
                store_index: inputs.store.index,
                store_index_writer: Some(self.fetching.store_index_writer),
                prefetched_cas_paths: Some(&inputs.prefetched.cas_paths),
                tarball_mem_cache: self.fetching.tarball_mem_cache,
                progress_reported: Some(self.fetching.progress_reported),
                verified_files_cache: inputs.store.verified_files_cache,
                custom_fetcher_session: self.fetching.custom_fetcher_session,
            },
            ctx: self.ctx,

            skipped: self.selection.skipped,
            include_optional_dependencies: self.selection.include_optional,
            runtime_platform_selector,

            // The slot link is deferred to the link thread of
            // `run_cold_batch` so it doesn't block this cooperative
            // task.
            defer_link: true,
            #[cfg(test)]
            link_concurrency_probe: self.link_concurrency_probe,
        }
    }
}

/// The resolution task is never cancelled, so a join error is its panic.
fn joined_resolution(
    joined: Result<Option<ResolvedSharedSideEffects>, tokio::task::JoinError>,
) -> Result<Option<ResolvedSharedSideEffects>, CreateVirtualStoreError> {
    Ok(joined.unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic())))
}

/// Spawned rather than joined with the downloads: the cold drain blocks
/// its task while the link thread runs (see `run_cold_batch`), so a
/// sibling future in that task would only run once the downloads end.
fn spawn_side_effects_resolution(
    prepared: Option<PreparedSharedSideEffects>,
    quarantine: &RemoteSideEffectsQuarantineBySnapshot,
) -> tokio::task::JoinHandle<Option<ResolvedSharedSideEffects>> {
    let quarantine = quarantine.clone();
    tokio::spawn(async move { resolve_prepared_side_effects(prepared, &quarantine).await })
}
