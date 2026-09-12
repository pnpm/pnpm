use super::{
    CasIndexes, CasPrefetch, CreateVirtualStore, CreateVirtualStoreError, CreateVirtualStoreOutput,
    CreateVirtualStoreStoreContext, LinkPlan, WantedEntries,
    cache_keys::SnapshotCacheKey,
    cold::{ColdBatch, ColdBatchState, ColdInputs, run_cold_batch},
    create_build_marker_source, init_store_dir_unless_frozen, nothing_to_materialize, partition,
    publish_planned_canonical_fetches, removed_aliases_by_key,
    slot_linking::LinkSlotsParallel,
    snapshot_plan,
    warm::{WarmLinkBatch, enforce_cached_git_prepare_policy, link_warm_batch},
};
use crate::{InstallPackageBySnapshot, install_package_by_snapshot::runtime_platform_selector};
use pnpm_config::NodeLinker;
use pnpm_lockfile::{LockfileEntries, PackageKey, PackageMetadata, PlatformSelector};
use pnpm_reporter::{LogEvent, LogLevel, Reporter, StatsLog, StatsMessage};
use pnpm_store_dir::{SharedVerifiedFilesCache, StoreDir};
use pnpm_tarball::PrefetchResult;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

impl<'a> CreateVirtualStore<'a> {
    /// Execute the subroutine. Returns the set of bundled manifests
    /// recovered from `index.db` for the warm-batch slots — the
    /// bin linker uses these to avoid re-reading `package.json` per
    /// child. See [`PackageManifests`](crate::create_virtual_store::PackageManifests).
    pub async fn run<Reporter: self::Reporter>(
        mut self,
    ) -> Result<CreateVirtualStoreOutput, CreateVirtualStoreError> {
        let Some(wanted) = self.wanted()? else {
            return Ok(nothing_to_materialize(self.is_hoisted()));
        };
        let prefetch = self.prefetch(wanted).await;
        let marker_source = self.prepare_store().await?;
        let mut plan = self.plan::<Reporter>(wanted, prefetch.cache_keys)?;
        let prefetched = self
            .settle_prefetch(
                prefetch.task,
                &prefetch.verified_files_cache,
                wanted.packages,
                &mut plan,
            )
            .await?;
        self.materialize_plan::<Reporter>(
            wanted,
            plan,
            &prefetched,
            CreateVirtualStoreStoreContext {
                index: prefetch.store_index.as_ref(),
                verified_files_cache: &prefetch.verified_files_cache,
            },
            marker_source.as_ref(),
        )
        .await
    }

    async fn materialize_plan<Reporter: self::Reporter>(
        &self,
        wanted: WantedEntries<'a>,
        plan: snapshot_plan::SnapshotPlan<'a>,
        prefetched: &PrefetchResult,
        store: CreateVirtualStoreStoreContext<'_>,
        marker_source: Option<&tempfile::NamedTempFile>,
    ) -> Result<CreateVirtualStoreOutput, CreateVirtualStoreError> {
        let mut partition = partition::partition_snapshots(
            &plan.survivors,
            &plan.skipped_entries,
            prefetched,
            &plan.marker_rebuilds,
            self.ctx.node_linker,
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
            self.planned_canonical_fetches,
            &partition.cold,
            wanted.packages,
            self.custom_fetcher_session.is_some(),
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
        let fetch_failed = self
            .download_cold::<Reporter>(
                ColdInputs { wanted, store, prefetched, marker_source, links: &links },
                &mut partition,
                &mut indexes,
            )
            .await?;
        self.apply_side_effects(wanted, &mut partition, &indexes.shared_base).await;

        // The writer is owned by the caller now. They drop their
        // sender and await the join handle after the build phase
        // finishes, so the final batch flushes after every queued
        // row from both the download path and the WRITE-path
        // upload.

        Ok(CreateVirtualStoreOutput {
            package_manifests: partition.package_manifests,
            side_effects_maps_by_snapshot: partition.side_effects_maps_by_snapshot,
            requires_build_by_snapshot: partition.requires_build_by_snapshot,
            materialized_snapshots: plan.materialized_keys(),
            fetch_failed,
            cas_paths_by_pkg_id: indexes.by_pkg_id,
        })
    }

    fn is_hoisted(&self) -> bool {
        matches!(self.ctx.node_linker, NodeLinker::Hoisted)
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
        match self.cas_prefetch.take() {
            Some(prefetch) => prefetch,
            None => {
                CasPrefetch::start(
                    self.ctx.config,
                    LockfileEntries {
                        packages: Some(wanted.packages),
                        snapshots: Some(wanted.snapshots),
                    },
                    self.supported_architectures,
                    self.store_context.as_ref(),
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
        if self.store_context.is_none() {
            init_store_dir_unless_frozen(config, store_dir).await;
        }
        create_build_marker_source(config, self.ctx.layout, store_dir)
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
        let plan = snapshot_plan::plan_snapshots::<Reporter>(snapshot_plan::SnapshotPlanInputs {
            snapshots: wanted.snapshots,
            packages: wanted.packages,
            current_entries: self.current_entries,
            layout: self.ctx.layout,
            allow_build_policy: self.ctx.allow_build_policy,
            skipped: self.skipped,
            link_dependencies: !self.is_hoisted() && config.symlink,
            force: config.force,
            is_hoisted: self.is_hoisted(),
            include_optional_dependencies: self.include_optional_dependencies,
            cache_keys: &mut cache_keys,
        })?;

        // `pnpm:stats added` fires one event per project once the
        // orchestrator has decided how many packages will land in the
        // virtual store. The value is the *delta* between current and
        // wanted lockfile, computed as the post-skip-filter snapshot
        // count so a warm reinstall against an unchanged lockfile
        // reports `added: 0`.
        //
        // The paired `pnpm:stats removed` event is emitted by the
        // caller from [`crate::PruneStaleModules`]'s result, so each
        // install carries exactly one `added` and one `removed`.
        //
        // Under the hoisted linker every snapshot survives the skip
        // filter (no slot to probe), and which packages are already on
        // disk is only known once its walker has run, so
        // [`crate::link_hoisted_modules()`] emits both stats there. A
        // `virtualStoreOnly` install never runs the linker, so it keeps
        // the count from here.
        if !self.is_hoisted() || self.ctx.config.virtual_store_only {
            Reporter::emit(&LogEvent::Stats(StatsLog {
                level: LogLevel::Debug,
                message: StatsMessage::Added {
                    prefix: self.ctx.requester.to_owned(),
                    added: plan.survivors.len() as u64,
                },
            }));
        }
        Ok(plan)
    }

    /// A joined-task failure degrades to an empty result: every lookup
    /// misses and the snapshots fall through to their per-snapshot
    /// path, the same shape as a store with no index.
    async fn settle_prefetch(
        &self,
        task: tokio::task::JoinHandle<PrefetchResult>,
        verified_files_cache: &SharedVerifiedFilesCache,
        packages: &HashMap<PackageKey, PackageMetadata>,
        plan: &mut snapshot_plan::SnapshotPlan<'_>,
    ) -> Result<PrefetchResult, CreateVirtualStoreError> {
        let prefetched = task.await.unwrap_or_else(|error| {
            tracing::warn!(
                target: "pacquet::install",
                ?error,
                "warm-cache prefetch task failed; treating every lookup as a miss",
            );
            PrefetchResult::default()
        });
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

    fn link_plan(&self, plan: &snapshot_plan::SnapshotPlan<'_>) -> LinkPlan<'a> {
        let config = self.ctx.config;
        LinkPlan {
            removed_aliases_by_key: removed_aliases_by_key(
                self.current_entries.snapshots,
                &plan.survivors,
            ),
            shared_packages: config
                .remote_side_effects_cache
                .as_ref()
                .map(|settings| settings.packages.iter().map(String::as_str).collect()),
            template: LinkSlotsParallel {
                batch: "warm",
                slots: &[],
                layout: self.ctx.layout,
                dir_clone_cache: self.dir_clone_cache,
                symlink: config.symlink,
                import_method: config.package_import_method,
                logged_methods: self.ctx.logged_methods,
                requester: self.ctx.requester,
                skipped: self.skipped,
                include_optional_dependencies: self.include_optional_dependencies,
                progress_reported: self.progress_reported,
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
                packages: wanted.packages,
                current_packages: self.current_entries.packages,
                is_hoisted: self.is_hoisted(),
                needs_build_marker_source: marker_source,
                removed_aliases_by_key: &links.removed_aliases_by_key,
                template: &links.template,
            },
        )
    }

    /// Snapshots that did not prefetch fall through to the tokio +
    /// download path. An optional snapshot whose fetch fails is dropped
    /// rather than aborting the install; the returned set holds those
    /// keys for the caller to fold into its [`crate::SkippedSnapshots`],
    /// so downstream walkers (`build_graph`, `link_bins`, hoist) treat
    /// the snapshot as absent. Under the hoisted linker no slot is
    /// written and each download's CAS index is the only output, folded
    /// into [`CasIndexes::by_pkg_id`]; the isolated linker's slot import
    /// has already happened by the time the download future returns.
    async fn download_cold<Reporter: self::Reporter>(
        &self,
        inputs: ColdInputs<'_, 'a>,
        partition: &mut partition::Partition<'_>,
        indexes: &mut CasIndexes,
    ) -> Result<HashSet<PackageKey>, CreateVirtualStoreError> {
        let runtime_platform_selector = runtime_platform_selector(self.supported_architectures);
        let mut fetch_failed = HashSet::new();
        let mut cold_cas_paths = Vec::new();
        run_cold_batch::<Reporter>(
            ColdBatch {
                cold: &partition.cold,
                installer: self.cold_installer(&inputs, &runtime_platform_selector),
                packages: inputs.wanted.packages,
                current_packages: self.current_entries.packages,
                marker_source: inputs.marker_source,
                removed_aliases_by_key: &inputs.links.removed_aliases_by_key,
                link_template: &inputs.links.template,
                shared_packages: inputs.links.shared_packages.as_ref(),
                is_hoisted: self.is_hoisted(),
            },
            &mut ColdBatchState {
                fetch_failed: &mut fetch_failed,
                requires_build_by_snapshot: &mut partition.requires_build_by_snapshot,
                shared_base_cas_paths: &mut indexes.shared_base,
            },
            &mut cold_cas_paths,
        )
        .await?;
        indexes.add_cold(cold_cas_paths);
        Ok(fetch_failed)
    }

    // Defer slot links to the parallel drain, outside the cooperative download tasks.
    fn cold_installer<'i>(
        &'i self,
        inputs: &'i ColdInputs<'_, 'a>,
        runtime_platform_selector: &'i PlatformSelector,
    ) -> InstallPackageBySnapshot<'i> {
        InstallPackageBySnapshot {
            ctx: self.ctx,
            http_client: self.http_client,
            store_index: inputs.store.index,
            store_index_writer: Some(self.store_index_writer),
            prefetched_cas_paths: Some(&inputs.prefetched.cas_paths),
            tarball_mem_cache: self.tarball_mem_cache,
            progress_reported: Some(self.progress_reported),
            verified_files_cache: inputs.store.verified_files_cache,
            skipped: self.skipped,
            include_optional_dependencies: self.include_optional_dependencies,
            runtime_platform_selector,
            custom_fetcher_session: self.custom_fetcher_session,
            // The slot link is deferred to the parallel pass in
            // `drain_cold_downloads` so it doesn't serialize
            // inside this cooperative task.
            defer_link: true,
            #[cfg(test)]
            link_concurrency_probe: self.link_concurrency_probe,
        }
    }

    async fn apply_side_effects(
        &self,
        wanted: WantedEntries<'a>,
        partition: &mut partition::Partition<'_>,
        base_cas_paths: &crate::shared_side_effects::BaseCasPaths,
    ) {
        crate::shared_side_effects::apply_shared_side_effects(
            crate::shared_side_effects::ApplySharedSideEffectsOptions {
                config: self.ctx.config,
                snapshots: wanted.snapshots,
                packages: wanted.packages,
                requires_build_by_snapshot: &partition.requires_build_by_snapshot,
                allow_build_policy: self.ctx.allow_build_policy,
                base_cas_paths,
                side_effects_maps_by_snapshot: &mut partition.side_effects_maps_by_snapshot,
                side_effects_by_snapshot: &partition.side_effects_by_snapshot,
                remote_side_effects_quarantine_by_snapshot: &partition
                    .remote_side_effects_quarantine_by_snapshot,
                store_index_keys_by_snapshot: &partition.store_index_keys_by_snapshot,
                store_index_writer: self.store_index_writer,
            },
        )
        .await;
    }
}
pub(super) fn imported_cache_keys(
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
