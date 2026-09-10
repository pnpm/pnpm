use super::{FreshInputs, errors::InstallWithFreshLockfileError, resolver_setup};
use crate::{CreateVirtualStore, CreateVirtualStoreOutput, SkippedSnapshots};
use pnpm_config::{Config, NodeLinker};
use pnpm_lockfile::{Lockfile, LockfileEntries};
use pnpm_package_manifest::PackageManifest;
use pnpm_reporter::{LogEvent, LogLevel, Reporter, Stage, StageLog};
use pnpm_tarball::MemCache;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
    sync::{Arc, atomic::AtomicU8},
};

/// Write the freshly-built wanted lockfile to `target`, first running the
/// `afterAllResolved` pnpmfile hook when one is configured.
///
/// `afterAllResolved` receives the resolved lockfile object and
/// returns the (possibly mutated) lockfile that gets written. The round-trip
/// goes through `serde_json::Value` so hook-added keys the typed [`Lockfile`]
/// cannot represent survive to disk; `serde_json`'s `preserve_order` feature
/// keeps the output byte-identical to the typed write when the hook makes no
/// changes. A throwing hook aborts the install.
/// The environment lifecycle scripts run under: `config.extra_env` plus
/// the `NODE_OPTIONS` for the selected project-level dependency loader.
pub(super) fn build_extra_env(
    config: &Config,
    node_linker: NodeLinker,
    workspace_root: &Path,
) -> HashMap<String, String> {
    let mut env = config.extra_env.clone();
    if let Some(node_options) = &config.node_options {
        env.insert("NODE_OPTIONS".to_string(), node_options.clone());
    }
    if matches!(node_linker, NodeLinker::Pnp) {
        let node_options = env.get("NODE_OPTIONS").map(String::as_str);
        env.insert(
            "NODE_OPTIONS".to_string(),
            crate::make_node_require_option(
                &workspace_root.join(crate::PNP_FILENAME),
                node_options,
            ),
        );
    }
    if config.node_experimental_package_map && !matches!(node_linker, NodeLinker::Pnp) {
        let package_map_path = config.modules_dir.join(crate::package_map::PACKAGE_MAP_FILENAME);
        let node_options = env.get("NODE_OPTIONS").map(String::as_str);
        env.insert(
            "NODE_OPTIONS".to_string(),
            crate::make_node_package_map_option(&package_map_path, node_options),
        );
    }
    env
}
/// The concurrent pre-resolve verification of the existing lockfile must have
/// its verdict before anything sensitive: the symlink / bin-link phases, the
/// dependency builds, and the lockfile save all run on a trusted lockfile
/// only.
pub(super) async fn await_lockfile_gate(
    gate: &mut Option<crate::install::LockfileVerificationGate>,
) -> Result<(), InstallWithFreshLockfileError> {
    let Some(gate) = gate.take() else { return Ok(()) };
    gate.wait().await.map_err(InstallWithFreshLockfileError::LockfileVerification)
}
/// What the on-disk phases read once the lockfile is built and the
/// materialization plan is fixed.
pub(super) struct OnDiskInputs<'a> {
    pub(super) install: FreshInputs<'a>,
    pub(super) ctx: &'a pnpm_deps_restorer::InstallContext<'a>,
    pub(super) include_transitive_optional_dependencies: bool,
    pub(super) deps_requiring_build_sink: Option<crate::DepsRequiringBuildSink>,
    pub(super) tarball_mem_cache: &'a Arc<MemCache>,
    pub(super) materialization_lockfile: &'a Lockfile,
    pub(super) importer_manifests: &'a std::collections::BTreeMap<String, &'a PackageManifest>,
    pub(super) project_anchor_importer_ids: &'a std::collections::HashSet<String>,
    pub(super) dir_clone_cache: Option<&'a pnpm_deps_restorer::DirCloneCache<'a>>,
    pub(super) host_node: Option<&'a pnpm_deps_restorer::materialization_plan::HostNode>,
    pub(super) engine_name: Option<String>,
    pub(super) deferred_engine_name:
        Option<pnpm_deps_restorer::materialization_plan::DeferredEngineName>,
    pub(super) patched_dependencies: Option<&'a pnpm_patching::PatchGroupRecord>,
    pub(super) custom_fetcher_session: Option<&'a Arc<pnpm_deps_restorer::CustomFetcherSession>>,
    pub(super) store_index_ref: Option<&'a pnpm_store_dir::SharedReadonlyStoreIndex>,
    pub(super) store_index_writer: Arc<pnpm_store_dir::StoreIndexWriter>,
    pub(super) caches: &'a resolver_setup::StoreCaches,
}
/// What the on-disk phases leave for the install's result.
pub(super) struct OnDiskOutput {
    pub(super) hoisted_dependencies: crate::HoistedDependencies,
    pub(super) hoisted_locations: std::collections::BTreeMap<String, Vec<String>>,
    pub(super) injected_deps: BTreeMap<String, Vec<String>>,
    pub(super) ignored_builds: Vec<String>,
    pub(super) deferred_builds: Vec<String>,
    pub(super) skipped: SkippedSnapshots,
}
impl<'a> OnDiskInputs<'a> {
    /// See `linking::run_link_phase` for why this anchors on
    /// `modules_dir.parent()` rather than the install root.
    fn symlink_root(&self) -> &'a Path {
        self.ctx.config.modules_dir.parent().unwrap_or(self.ctx.workspace_root)
    }

    /// Materialize the virtual store. Skipped snapshots stay out of every
    /// map the output carries; the fetch failures it reports are folded into
    /// the skip set by the caller before anything links.
    async fn materialize<Reporter: self::Reporter + 'static>(
        &self,
        skipped: &SkippedSnapshots,
    ) -> Result<CreateVirtualStoreOutput, InstallWithFreshLockfileError> {
        let phase_start = std::time::Instant::now();
        let materialized = CreateVirtualStore {
            ctx: self.ctx,
            http_client: self.install.http_client,
            entries: self.materialization_lockfile.into(),
            current_entries: LockfileEntries::of_previous_install(
                self.install.current_lockfile,
                self.ctx.config.force,
            ),
            store_index_writer: &self.store_index_writer,
            store_context: Some(pnpm_deps_restorer::CreateVirtualStoreStoreContext {
                index: self.store_index_ref,
                verified_files_cache: &self.caches.verified_files,
            }),
            cas_prefetch: None,
            skipped,
            include_optional_dependencies: self.include_transitive_optional_dependencies,
            supported_architectures: self.install.supported_architectures,
            dir_clone_cache: self.dir_clone_cache,
            progress_reported: &self.caches.progress_reported,
            // Share the resolve-time prefetcher's in-flight downloads with
            // the cold batch. The `PrefetchingResolver` streams each
            // tarball into `tarball_mem_cache` keyed by URL; the cold
            // batch's only on-disk dedup is the store-index row, which the
            // prefetcher's writer commits asynchronously. Without the
            // shared cache a snapshot whose prefetch hasn't committed its
            // row yet is classified cold and re-downloaded — a race that
            // routing the cold batch through the mem cache fixes by
            // reusing the in-flight download instead.
            tarball_mem_cache: Some(self.tarball_mem_cache),
            custom_fetcher_session: self.custom_fetcher_session,
            // The fresh path's concurrent gate verifies the *previous*
            // lockfile while this run fetches the new graph; the two
            // entry sets differ, so no fetch plan is published and the
            // verifier keeps its metadata-backed path.
            planned_canonical_fetches: None,
        }
        .run::<Reporter>()
        .await
        .map_err(InstallWithFreshLockfileError::CreateVirtualStore)?;
        tracing::info!(
            target: "pacquet::install::phase",
            phase = "create_virtual_store",
            elapsed_ms = phase_start.elapsed().as_millis() as u64,
            "phase complete",
        );
        Ok(materialized)
    }

    fn project_manifests(&self, only_anchors: bool) -> Vec<(std::path::PathBuf, &PackageManifest)> {
        self.importer_manifests
            .iter()
            .filter(|(id, _)| {
                !only_anchors || self.project_anchor_importer_ids.contains(id.as_str())
            })
            .map(|(id, manifest)| (self.ctx.workspace_root.join(id), *manifest))
            .collect()
    }

    fn root_component_importers(&self) -> std::collections::HashSet<String> {
        self.importer_manifests
            .iter()
            .filter(|(id, _)| self.project_anchor_importer_ids.contains(id.as_str()))
            .filter(|(_, manifest)| {
                manifest.install_config_hoisting_limits()
                    == Some(pnpm_deps_restorer::HOISTING_LIMITS_WORKSPACES)
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Link the materialized store into every project and report
    /// `importing_done`, which reporters use to close the import progress
    /// display before the `pnpm:lifecycle` events of the build.
    fn link<Reporter: self::Reporter + 'static>(
        &self,
        materialized: &mut CreateVirtualStoreOutput,
        skipped: &mut SkippedSnapshots,
    ) -> Result<pnpm_deps_restorer::linking::LinkPhaseOutput, InstallWithFreshLockfileError> {
        let project_manifests = self.project_manifests(true);
        let package_map_project_manifests = self.project_manifests(false);
        let root_component_importers = self.root_component_importers();

        let linked = pnpm_deps_restorer::linking::run_link_phase::<Reporter>(
            pnpm_deps_restorer::linking::LinkPhaseInputs {
                ctx: self.ctx,
                requires_build_by_snapshot: None,
                symlink_root: self.symlink_root(),
                trusted_importer_ids: self.project_anchor_importer_ids,
                root_component_importers: &root_component_importers,
                sidecar_lockfile: self.materialization_lockfile,
                lockfile: self.materialization_lockfile,
                current_lockfile: self.install.current_lockfile,
                materialized_snapshots: Some(&materialized.materialized_snapshots),
                project_manifests: &project_manifests,
                package_map_project_manifests: &package_map_project_manifests,
                dependency_groups: self.install.dependency_groups,
                package_manifests: &materialized.package_manifests,
                cas_paths_by_pkg_id: materialized.cas_paths_by_pkg_id.take(),
                prune_orphans: self.install.prune_orphans,
                prior_hoisted_dependencies: self.install.prior_hoisted_dependencies,
                prior_hoisted_locations: self.install.prior_hoisted_locations,
                // `pnpm rebuild` takes the frozen path; only an `allowBuilds`
                // change asks for present packages here.
                build_present_packages: self.install.allow_builds_changed,
                prior_unbuilt_builds: self.install.prior_unbuilt_builds,
                host_node: self.host_node,
                supported_architectures: self.install.supported_architectures,
            },
            skipped,
        )
        .map_err(InstallWithFreshLockfileError::LinkPhase)?;
        Reporter::emit(&LogEvent::Stage(StageLog {
            level: LogLevel::Debug,
            prefix: self.ctx.requester.to_string(),
            stage: Stage::ImportingDone,
        }));
        Ok(linked)
    }

    /// Run lifecycle scripts, report ignored builds, and re-link top-level
    /// bins: the build phase the frozen path runs, so `pacquet add esbuild`
    /// reports the blocked `esbuild` build (and builds approved packages)
    /// exactly like `pnpm`. Scripts see the install root as `INIT_CWD`; the
    /// post-build bin link anchors on [`Self::symlink_root`], where this
    /// path placed `node_modules`.
    ///
    /// Consumes the inputs: the store-index writer handle is dropped once
    /// the side-effects-cache rows are queued, so the channel closes and
    /// the writer task starts winding down while the caller finishes. The
    /// install driver awaits it as `store_index_teardown`.
    async fn build<Reporter: self::Reporter + 'static>(
        self,
        materialized: &CreateVirtualStoreOutput,
        linked: &pnpm_deps_restorer::linking::LinkPhaseOutput,
        skipped: &SkippedSnapshots,
    ) -> Result<crate::BuildModulesOutput, InstallWithFreshLockfileError> {
        // Resolve the deferred `node --version` probe (non-GVS path); it
        // overlapped `CreateVirtualStore`. Falls back to the synchronous
        // value when the probe wasn't deferred.
        let top_level_bin_root = self.symlink_root();
        let engine_name = settle_engine_name(self.deferred_engine_name, self.engine_name).await;
        let extra_env =
            build_extra_env(self.ctx.config, self.ctx.node_linker, self.ctx.workspace_root);
        publish_deps_requiring_build(
            self.deps_requiring_build_sink.as_ref(),
            &materialized.requires_build_by_snapshot,
        );
        let built = crate::install_frozen_lockfile::run_build_phase::<Reporter>(
            &crate::install_frozen_lockfile::BuildPhaseInputs {
                config: self.ctx.config,
                workspace_root: self.ctx.workspace_root,
                top_level_bin_root,
                layout: self.ctx.layout,
                snapshots: self.materialization_lockfile.snapshots.as_ref(),
                packages: self.materialization_lockfile.packages.as_ref(),
                importers: &self.materialization_lockfile.importers,
                dependency_groups: self.install.dependency_groups,
                // Reuse the record resolved earlier for the resolver so the
                // patch files aren't hashed a second time.
                patch_groups: self.patched_dependencies,
                allow_build_policy: self.ctx.allow_build_policy,
                side_effects_maps_by_snapshot: &materialized.side_effects_maps_by_snapshot,
                requires_build_by_snapshot: &materialized.requires_build_by_snapshot,
                materialized_snapshots: linked
                    .build_snapshots(&materialized.materialized_snapshots),
                engine_name: engine_name.as_deref(),
                extra_env: &extra_env,
                store_index_writer: &self.store_index_writer,
                skipped,
                hoisted_pkg_roots_by_key: linked.hoisted_pkg_roots_by_key.as_ref(),
                is_hoisted: self.ctx.is_hoisted(),
                publicly_hoisted_for_post_build: &linked.publicly_hoisted_for_post_build,
                logged_methods: self.ctx.logged_methods,
                // The fresh-resolve path never serves an explicit
                // `pacquet rebuild`; rebuilds always take the frozen path.
                rebuild: None,
                link_options: self.ctx.link_options,
            },
        )
        .map_err(InstallWithFreshLockfileError::BuildPhase)?;
        drop(self.store_index_writer);
        Ok(built)
    }
}
/// Materialize the virtual store, link it into every project, and run the
/// dependency builds. The lockfile is persisted by the caller, after this
/// returns, so a partial install cannot leave one behind.
pub(super) async fn run_on_disk_phases<Reporter: self::Reporter + 'static>(
    inputs: OnDiskInputs<'_>,
    skipped: &mut SkippedSnapshots,
    lockfile_verification_gate: &mut Option<crate::LockfileVerificationGate>,
) -> Result<OnDiskOutput, InstallWithFreshLockfileError> {
    let ctx = inputs.ctx;
    let lockfile = inputs.materialization_lockfile;
    let mut materialized = inputs.materialize::<Reporter>(skipped).await?;

    // The concurrent pre-resolve verification of the existing
    // lockfile must have its verdict before anything sensitive: the
    // symlink / bin-link phases, the dependency builds, and the
    // lockfile save below all run on a trusted lockfile only.
    await_lockfile_gate(lockfile_verification_gate).await?;
    fold_fetch_failures(skipped, std::mem::take(&mut materialized.fetch_failed));

    let linked = inputs.link::<Reporter>(&mut materialized, skipped)?;
    let crate::BuildModulesOutput { ignored_builds, deferred_builds, mutated_slots: _ } =
        inputs.build::<Reporter>(&materialized, &linked, skipped).await?;

    let injected_deps = crate::collect_injected_deps(
        ctx.layout,
        ctx.workspace_root,
        lockfile.into(),
        skipped,
        ctx.is_hoisted().then_some(&linked.hoisted_locations),
    );
    Ok(OnDiskOutput {
        hoisted_dependencies: linked.hoisted_dependencies,
        hoisted_locations: linked.hoisted_locations,
        injected_deps,
        ignored_builds,
        deferred_builds,
        skipped: std::mem::take(skipped),
    })
}
pub(super) async fn finish_early_materialization<Reporter: self::Reporter + 'static>(
    materializer: Option<&crate::early_materializer::EarlyMaterializer<Reporter>>,
    wanted: Option<&HashMap<pnpm_lockfile::PackageKey, pnpm_lockfile::SnapshotEntry>>,
    skipped: &SkippedSnapshots,
    logged_methods: &AtomicU8,
) {
    let Some(materializer) = materializer else { return };
    let phase_start = std::time::Instant::now();
    let materialized = materializer
        .finish(
            |key| {
                wanted.is_some_and(|snapshots| snapshots.contains_key(key))
                    && !skipped.contains(key)
            },
            logged_methods,
        )
        .await;
    tracing::info!(
        target: "pacquet::install::phase",
        phase = "early_materialization",
        elapsed_ms = phase_start.elapsed().as_millis() as u64,
        materialized,
        "phase complete",
    );
}
pub(super) fn fold_fetch_failures(
    skipped: &mut SkippedSnapshots,
    fetch_failed: HashSet<pnpm_lockfile::PackageKey>,
) {
    for key in fetch_failed {
        skipped.add_fetch_failed(key);
    }
}
/// `CreateVirtualStore` keeps skipped snapshots out of this map, so it holds
/// only what the install put on disk. See [`crate::DepsRequiringBuildSink`].
pub(super) fn publish_deps_requiring_build(
    sink: Option<&crate::DepsRequiringBuildSink>,
    requires_build_by_snapshot: &HashMap<pnpm_lockfile::PackageKey, bool>,
) {
    let Some(sink) = sink else { return };
    let deps_requiring_build = requires_build_by_snapshot
        .iter()
        .filter(|(_, requires_build)| **requires_build)
        .map(|(snapshot_key, _)| snapshot_key.to_string())
        .collect();
    *sink.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(deps_requiring_build);
}
/// Resolve the deferred `node --version` probe (non-GVS path); it overlapped
/// `CreateVirtualStore`. Falls back to the synchronous value when the probe
/// wasn't deferred.
pub(super) async fn settle_engine_name(
    deferred: Option<pnpm_deps_restorer::materialization_plan::DeferredEngineName>,
    engine_name: Option<String>,
) -> Option<String> {
    match deferred {
        Some(deferred) => deferred.handle.await.ok().flatten(),
        None => engine_name,
    }
}
