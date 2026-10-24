use super::{
    CasIndexes, CasPrefetch, ConcurrentCheck, CreateVirtualStore, CreateVirtualStoreError,
    CreateVirtualStoreOutput, CreateVirtualStoreStoreContext, LinkPlan, PrefetchTask,
    WantedEntries,
    cache_keys::{SlotReuse, SnapshotCacheKey},
    cas::{retain_fetch_pass_rows, retained_cache_keys},
    cold::ColdInputs,
    create_build_marker_source, init_store_dir_unless_frozen, nothing_to_materialize, partition,
    publish_planned_canonical_fetches, removed_aliases_by_key, run_checked,
    slot_linking::LinkSlotsParallel,
    snapshot_plan,
    warm::{WarmLinkBatch, link_warm_batch},
};

mod downloads;
use pnpm_config::NodeLinker;
use pnpm_lockfile::{LockfileEntries, PackageKey, PackageMetadata, SnapshotEntry};
use pnpm_reporter::{LogEvent, LogLevel, Reporter, StatsLog, StatsMessage};
use pnpm_store_dir::{SharedReadonlyStoreIndex, SharedVerifiedFilesCache};
use pnpm_tarball::PrefetchResult;
use std::{collections::HashMap, path::Path};

impl<'a> CreateVirtualStore<'a> {
    /// Execute the subroutine. Returns the set of bundled manifests
    /// recovered from `index.db` for the warm-batch slots — the
    /// bin linker uses these to avoid re-reading `package.json` per
    /// child. See [`PackageManifests`](crate::create_virtual_store::PackageManifests).
    pub(super) async fn run_inner<Reporter: self::Reporter>(
        &mut self,
        check: &mut Option<ConcurrentCheck<'_>>,
    ) -> Result<CreateVirtualStoreOutput, CreateVirtualStoreError> {
        // Boxed for `clippy::large_futures`: every install awaits this future.
        Box::pin(self.run_retaining::<Reporter>(None, check)).await.map(|(output, _)| output)
    }

    /// [`Self::run_inner`], also returning the settled prefetch of
    /// `retained` for a later pass over those snapshots. See
    /// [`retain_fetch_pass_rows`].
    pub(super) async fn run_retaining<Reporter: self::Reporter>(
        &mut self,
        retained: Option<&HashMap<PackageKey, SnapshotEntry>>,
        check: &mut Option<ConcurrentCheck<'_>>,
    ) -> Result<(CreateVirtualStoreOutput, Option<CasPrefetch>), CreateVirtualStoreError> {
        let Some(wanted) = self.wanted()? else {
            return Ok((nothing_to_materialize(self.is_hoisted()), None));
        };
        let (prefetch, marker_source) = self.prefetch_and_prepare_store(wanted, check).await?;
        let retained_keys =
            retained.map(|snapshots| retained_cache_keys(&prefetch.cache_keys, snapshots));
        let mut plan = self.plan::<Reporter>(wanted, prefetch.cache_keys)?;
        let planned = plan.survivors.len();
        let prefetched = run_checked(
            check,
            self.settle_prefetch(
                prefetch.task,
                &prefetch.verified_files_cache,
                wanted.packages,
                &mut plan,
            ),
        )
        .await?;
        let output = self.materialize_plan::<Reporter>(
            wanted,
            plan,
            &prefetched,
            CreateVirtualStoreStoreContext {
                index: prefetch.store_index.as_ref(),
                verified_files_cache: &prefetch.verified_files_cache,
            },
            marker_source.as_ref(),
            check,
        )
        .await?;
        self.report_added::<Reporter>(planned.saturating_sub(output.fetch_failed.len()));
        let handoff = retained_keys.map(|cache_keys| {
            retained_handoff(
                (prefetch.store_index, prefetch.verified_files_cache),
                prefetched,
                cache_keys,
                &output,
            )
        });
        Ok((output, handoff))
    }

    async fn prefetch_and_prepare_store(
        &mut self,
        wanted: WantedEntries<'a>,
        check: &mut Option<ConcurrentCheck<'_>>,
    ) -> Result<(CasPrefetch, Option<tempfile::NamedTempFile>), CreateVirtualStoreError> {
        let prefetch = run_checked(check, async { Ok(self.prefetch(wanted).await) }).await?;
        let marker_source = run_checked(check, self.prepare_store()).await?;
        Ok((prefetch, marker_source))
    }

    /// `pnpm:stats added` fires one event per project once the
    /// downloads have settled. The value is the *delta* between current
    /// and wanted lockfile, computed as the post-skip-filter snapshot
    /// count less the optional snapshots whose fetch failed, so a warm
    /// reinstall against an unchanged lockfile reports `added: 0` and a
    /// skipped optional package is not counted.
    ///
    /// The paired `pnpm:stats removed` event is emitted by the
    /// caller from [`crate::PruneStaleModules`]'s result, so each
    /// install carries exactly one `added` and one `removed`.
    ///
    /// Under the hoisted linker every snapshot survives the skip
    /// filter (no slot to probe), and which packages are already on
    /// disk is only known once its walker has run, so
    /// [`crate::link_hoisted_modules()`] emits both stats there. A
    /// `virtualStoreOnly` install never runs the linker, so it keeps
    /// the count from here.
    fn report_added<Reporter: self::Reporter>(&self, added: usize) {
        if self.is_hoisted() && !self.ctx.config.virtual_store_only {
            return;
        }
        Reporter::emit(&LogEvent::Stats(StatsLog {
            level: LogLevel::Debug,
            message: StatsMessage::Added {
                prefix: self.ctx.requester.to_owned(),
                added: added as u64,
            },
        }));
    }

    fn rebuilds<'p>(
        &'p self,
        plan: &'p snapshot_plan::SnapshotPlan<'a>,
    ) -> partition::Rebuilds<'p> {
        partition::Rebuilds {
            markers: &plan.marker_rebuilds,
            allow_build_policy: self.ctx.allow_build_policy,
            known_requires_build: self.selection.known_requires_build,
            known_bins: self.selection.known_bins,
        }
    }

    async fn materialize_plan<Reporter: self::Reporter>(
        &self,
        wanted: WantedEntries<'a>,
        plan: snapshot_plan::SnapshotPlan<'a>,
        prefetched: &PrefetchResult,
        store: CreateVirtualStoreStoreContext<'_>,
        marker_source: Option<&tempfile::NamedTempFile>,
        check: &mut Option<ConcurrentCheck<'_>>,
    ) -> Result<CreateVirtualStoreOutput, CreateVirtualStoreError> {
        let mut partition = partition::partition_snapshots(
            &plan.survivors,
            &plan.skipped_entries,
            prefetched,
            &self.rebuilds(&plan),
            self.ctx.linker.kind,
        );

        // Publish the cold-batch fetch plan for the concurrent
        // verification fan-out: every cold registry-resolved snapshot
        // with a pinned hash is downloaded from its canonical registry
        // URL by this run (or fails the install / is dropped as an
        // uninstallable optional), which is the existence evidence the
        // npm verifier's age gate may substitute for a metadata body.
        // First fill wins; entries outside the plan keep the
        // metadata-backed path.
        publish_planned_canonical_fetches(
            self.fetching.planned_canonical_fetches,
            &partition.cold,
            wanted.packages,
            self.fetching.custom_fetcher_session.is_some(),
        );

        let links = self.link_plan(&plan);
        let mut indexes =
            CasIndexes::warm(links.shared_packages.as_ref(), &partition.warm, self.is_hoisted());
        self.link_warm::<Reporter>(
            wanted,
            &partition,
            &links,
            marker_source.map(tempfile::NamedTempFile::path),
        )?;
        let fetch_failed = self.download_and_apply_side_effects::<Reporter>(
            ColdInputs { wanted, store, prefetched, marker_source, links: &links },
            (&mut partition, &mut indexes),
            check,
        )
        .await?;

        // The writer is owned by the caller now. They drop their
        // sender and await the join handle after the build phase
        // finishes, so the final batch flushes after every queued
        // row from both the download path and the WRITE-path
        // upload.

        Ok(CreateVirtualStoreOutput {
            package_manifests: partition.package_manifests,
            package_bins: partition.package_bins,
            side_effects_maps_by_snapshot: partition.side_effects.maps,
            side_effects_by_snapshot: partition.side_effects.diffs,
            requires_build_by_snapshot: partition.requires_build_by_snapshot,
            materialized_snapshots: plan.materialized_keys(),
            fetch_failed,
            cas_paths_by_pkg_id: indexes.by_pkg_id,
        })
    }

    fn is_hoisted(&self) -> bool {
        matches!(self.ctx.linker.kind, NodeLinker::Hoisted)
    }

    fn slot_reuse(&self, packages: &'a HashMap<PackageKey, PackageMetadata>) -> SlotReuse<'a> {
        SlotReuse {
            packages,
            current_packages: self.current_entries.packages,
            force: self.ctx.config.force,
        }
    }

    fn wanted(&self) -> Result<Option<WantedEntries<'a>>, CreateVirtualStoreError> {
        // No snapshots to install. If the lockfile also has no project deps
        // this is a valid no-op; if it does, pnpm would have populated
        // `snapshots`, so bailing out here is safe enough for v9.
        let Some(snapshots) = self.entries.snapshots else { return Ok(None) };
        let packages =
            self.entries.packages.ok_or(CreateVirtualStoreError::MissingPackagesSection)?;
        Ok(Some(WantedEntries { packages, snapshots }))
    }

    async fn prefetch(&mut self, wanted: WantedEntries<'a>) -> CasPrefetch {
        match self.fetching.cas_prefetch.take() {
            Some(prefetch) => prefetch,
            None => {
                CasPrefetch::start(
                    self.ctx.config,
                    LockfileEntries {
                        packages: Some(wanted.packages),
                        snapshots: Some(wanted.snapshots),
                    },
                    self.ctx.allow_build_policy,
                    self.selection.supported_architectures,
                    self.fetching.store_context.as_ref(),
                )
                .await
            }
        }
    }

    async fn prepare_store(
        &self,
    ) -> Result<Option<tempfile::NamedTempFile>, CreateVirtualStoreError> {
        let config = self.ctx.config;
        let store_dir: &'static _ = &config.store_dir;
        if self.fetching.store_context.is_none() {
            init_store_dir_unless_frozen(config, store_dir).await;
        }
        create_build_marker_source(config, self.ctx.linker.layout, store_dir)
    }

    /// The plan pass consumes the keys [`CasPrefetch::start`] derived
    /// (one per snapshot, kept as `Result`s so the strict/lenient
    /// asymmetry documented on `plan_snapshots` survives the early
    /// derivation), stashing each survivor's key alongside its
    /// `(snapshot_key, snapshot)` tuple for the warm/cold partition.
    /// Its slot probes run while the prefetch task reads the store
    /// index.
    fn plan<Reporter: self::Reporter>(
        &self,
        wanted: WantedEntries<'a>,
        mut cache_keys: HashMap<PackageKey, Result<SnapshotCacheKey, CreateVirtualStoreError>>,
    ) -> Result<snapshot_plan::SnapshotPlan<'a>, CreateVirtualStoreError> {
        let config = self.ctx.config;
        snapshot_plan::plan_snapshots::<Reporter>(snapshot_plan::SnapshotPlanInputs {
            policy: crate::create_virtual_store::snapshot_plan::SnapshotReusePolicy {
                skipped: self.selection.skipped,
                link_dependencies: !self.is_hoisted() && config.symlink,
                force: config.force,
                is_hoisted: self.is_hoisted(),
                include_optional: self.selection.include_optional,
            },
            snapshots: wanted.snapshots,
            packages: wanted.packages,
            current_entries: self.current_entries,
            layout: self.ctx.linker.layout,
            allow_build_policy: self.ctx.allow_build_policy,

            cache_keys: &mut cache_keys,
        })
    }

    fn link_plan(&self, plan: &snapshot_plan::SnapshotPlan<'_>) -> LinkPlan<'a> {
        let config = self.ctx.config;
        LinkPlan {
            removed_aliases_by_key: removed_aliases_by_key(
                self.current_entries.snapshots,
                &plan.survivors,
            ),
            shared_packages: config.remote_side_effects_cache
                .as_ref()
                .map(|settings| {
                    settings.packages
                        .iter()
                        .map(String::as_str)
                        .collect()
                }),
            template: LinkSlotsParallel {
                import: crate::PackageImportOptions::from_config(
                    config,
                    self.ctx.caches.logged_methods,
                    self.ctx.requester,
                    self.ctx.caches.swap_lock,
                ),
                link: crate::VirtualStoreLinkOptions {
                    layout: self.ctx.linker.layout,
                    dir_clone_cache: self.dir_clone_cache,
                    symlink: config.symlink,
                    skipped: self.selection.skipped,
                    include_optional: self.selection.include_optional,
                },
                batch: "warm",
                slots: &[],

                progress_reported: self.fetching.progress_reported,
                #[cfg(test)]
                link_concurrency_probe: self.link_concurrency_probe,
            },
        }
    }

    fn link_warm<Reporter: self::Reporter>(
        &self,
        wanted: WantedEntries<'a>,
        partition: &partition::Partition<'_>,
        links: &LinkPlan<'_>,
        marker_source: Option<&Path>,
    ) -> Result<(), CreateVirtualStoreError> {
        link_warm_batch::<Reporter>(
            &partition.warm,
            &WarmLinkBatch {
                reuse: self.slot_reuse(wanted.packages),
                is_hoisted: self.is_hoisted(),
                needs_build_marker_source: marker_source,
                removed_aliases_by_key: &links.removed_aliases_by_key,
                template: &links.template,
            },
        )
    }
}

/// The settled prefetch of the retained snapshots, for a later pass.
fn retained_handoff(
    (store_index, verified_files_cache): (
        Option<SharedReadonlyStoreIndex>,
        SharedVerifiedFilesCache,
    ),
    mut rows: PrefetchResult,
    cache_keys: HashMap<PackageKey, Result<SnapshotCacheKey, CreateVirtualStoreError>>,
    output: &CreateVirtualStoreOutput,
) -> CasPrefetch {
    retain_fetch_pass_rows(&mut rows, &cache_keys, output);
    CasPrefetch {
        store_index,
        verified_files_cache,
        cache_keys,
        task: PrefetchTask::Settled(Box::new(rows)),
    }
}
