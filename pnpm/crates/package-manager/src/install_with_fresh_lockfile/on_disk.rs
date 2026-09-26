mod building;

use super::{FreshInputs, errors::InstallWithFreshLockfileError, resolver_setup};
use crate::{CreateVirtualStore, CreateVirtualStoreOutput, SkippedSnapshots};
use pnpm_lockfile::{Lockfile, LockfileEntries};
use pnpm_package_manifest::PackageManifest;
use pnpm_reporter::{LogEvent, LogLevel, Reporter, Stage, StageLog};
use pnpm_tarball::MemCache;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{Arc, atomic::AtomicU8},
};
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
    pub(super) patched_dependencies: Option<&'a pnpm_patching::PatchGroupRecord>,
    pub(super) store: OnDiskStore<'a>,
    pub(super) runtime: OnDiskRuntime<'a>,
    pub(super) projects: OnDiskProjects<'a>,
}

pub(super) struct OnDiskStore<'a> {
    pub(super) tarball_mem_cache: &'a Arc<MemCache>,
    pub(super) dir_clone_cache: Option<&'a pnpm_deps_restorer::DirCloneCache<'a>>,
    pub(super) custom_fetcher_session: Option<&'a Arc<pnpm_deps_restorer::CustomFetcherSession>>,
    pub(super) store_index_ref: Option<&'a pnpm_store_dir::SharedReadonlyStoreIndex>,
    pub(super) store_index_writer: Arc<pnpm_store_dir::StoreIndexWriter>,
    pub(super) caches: &'a resolver_setup::StoreCaches,
    pub(super) resolution_observer: Option<&'a dyn crate::ResolutionObserver>,
}

pub(super) struct OnDiskRuntime<'a> {
    pub(super) host_node: Option<&'a pnpm_deps_restorer::materialization_plan::HostNode>,
    pub(super) engine_name: Option<String>,
    pub(super) deferred_engine_name:
        Option<pnpm_deps_restorer::materialization_plan::DeferredEngineName>,
}

pub(super) struct OnDiskProjects<'a> {
    pub(super) materialization_lockfile: &'a Lockfile,
    pub(super) importer_manifests: &'a std::collections::BTreeMap<String, &'a PackageManifest>,
    pub(super) project_anchor_importer_ids: &'a std::collections::HashSet<String>,
}

/// What the on-disk phases leave for the install's result.
pub(super) struct OnDiskOutput {
    pub hoisted: pnpm_deps_restorer::InstalledHoistedState,
    pub(super) ignored_builds: Vec<String>,
    pub(super) deferred_builds: Vec<String>,
    pub(super) skipped: SkippedSnapshots,
}
impl<'a> OnDiskInputs<'a> {
    /// See `linking::run_link_phase` for why this anchors on
    /// `Config::modules_dir_anchor` rather than the install root.
    pub(super) fn symlink_root(&self) -> &'a Path {
        self.ctx.config.modules_dir_anchor().unwrap_or(self.ctx.workspace_root)
    }

    /// Materialize the virtual store. Skipped snapshots stay out of every
    /// map the output carries; the fetch failures it reports are folded into
    /// the skip set by the caller before anything links.
    async fn materialize<Reporter: self::Reporter + 'static>(
        &self,
        skipped: &SkippedSnapshots,
    ) -> Result<CreateVirtualStoreOutput, InstallWithFreshLockfileError> {
        if self.ctx.config.package_provider.is_some() {
            return Ok(CreateVirtualStoreOutput::default());
        }
        let phase_start = std::time::Instant::now();
        let materialized = CreateVirtualStore {
            fetching: pnpm_deps_restorer::VirtualStoreFetchInputs {
                http_client: self.install.drivers.http_client,
                store_index_writer: &self.store.store_index_writer,
                store_context: Some(pnpm_deps_restorer::CreateVirtualStoreStoreContext {
                    index: self.store.store_index_ref,
                    verified_files_cache: &self.store.caches.verified_files,
                }),
                cas_prefetch: None,
                progress_reported: &self.store.caches.progress_reported,
                tarball_mem_cache: Some(self.store.tarball_mem_cache),
                custom_fetcher_session: self.store.custom_fetcher_session,
                planned_canonical_fetches: None,
            },
            selection: pnpm_deps_restorer::SnapshotSelection {
                skipped,
                include_optional: self.include_transitive_optional_dependencies,
                supported_architectures: self.install.projects.supported_architectures,
            },
            ctx: self.ctx,

            entries: self.projects.materialization_lockfile.into(),
            current_entries: LockfileEntries::of_previous_install(self.install.prior.lockfile),
            importers: &self.projects.materialization_lockfile.importers,

            dir_clone_cache: self.store.dir_clone_cache,
            // Share the resolve-time prefetcher's in-flight downloads with
            // the cold batch. The `PrefetchingResolver` streams each
            // tarball into `tarball_mem_cache`; the cold
            // batch's only on-disk dedup is the store-index row, which the
            // prefetcher's writer commits asynchronously. Without the
            // shared cache a snapshot whose prefetch hasn't committed its
            // row yet is classified cold and re-downloaded — a race that
            // routing the cold batch through the mem cache fixes by
            // reusing the in-flight download instead.

            // The fresh path's concurrent gate verifies the *previous*
            // lockfile while this run fetches the new graph; the two
            // entry sets differ, so no fetch plan is published and the
            // verifier keeps its metadata-backed path.
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

    /// The graphs the link phase reconciles. A tree that moved with its
    /// project relinks every slot's bins, not only the materialized ones.
    fn link_lockfiles<'b>(
        &self,
        materialized_snapshots: &'b [pnpm_lockfile::PackageKey],
    ) -> pnpm_deps_restorer::LinkLockfiles<'b>
    where
        'a: 'b,
    {
        pnpm_deps_restorer::LinkLockfiles {
            lockfile: self.projects.materialization_lockfile,
            current_lockfile: self.install.prior.lockfile,
            materialized_snapshots: (!self.install.prior.relink_every_slot_bin).then_some(
                materialized_snapshots,
            ),
            sidecar_lockfile: self.projects.materialization_lockfile,
        }
    }

    /// Link the materialized store into every project and report
    /// `importing_done`, which reporters use to close the import progress
    /// display before the `pnpm:lifecycle` events of the build.
    fn link<Reporter: self::Reporter + 'static>(
        &self,
        materialized: &mut CreateVirtualStoreOutput,
        skipped: &mut SkippedSnapshots,
    ) -> Result<pnpm_deps_restorer::linking::LinkPhaseOutput, InstallWithFreshLockfileError> {
        let project_manifests = self.projects.project_manifests(self.ctx.workspace_root, true);
        let package_map_project_manifests =
            self.projects.project_manifests(self.ctx.workspace_root, false);
        let root_component_importers = self.projects.root_component_importers();

        let linked = pnpm_deps_restorer::linking::run_link_phase::<Reporter>(
            pnpm_deps_restorer::linking::LinkPhaseInputs {
                graph: self.link_lockfiles(&materialized.materialized_snapshots),
                packages: pnpm_deps_restorer::LinkPackageData {
                    package_manifests: &materialized.package_manifests,
                    requires_build_by_snapshot: None,
                    cas_paths_by_pkg_id: materialized.cas_paths_by_pkg_id.take(),
                },
                prior: self.install.prior.link_state(),
                projects: pnpm_deps_restorer::LinkProjects {
                    manifests: &project_manifests,
                    package_map_manifests: &package_map_project_manifests,
                    dependency_groups: self.install.projects.dependency_groups,
                    symlink_root: self.symlink_root(),
                    trusted_importer_ids: self.projects.project_anchor_importer_ids,
                    root_component_importers: &root_component_importers,
                },
                ctx: self.ctx,

                host_node: self.runtime.host_node,
                supported_architectures: self.install.projects.supported_architectures,
            },
            skipped,
        )
        .map_err(InstallWithFreshLockfileError::LinkPhase)?;
        if let Some(observer) = self.store.resolution_observer {
            observer.flush_progress();
        }
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
        self::building::run_build::<Reporter>(self, materialized, linked, skipped).await
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
    let lockfile = inputs.projects.materialization_lockfile;
    let mut materialized = inputs.materialize::<Reporter>(skipped).await?;

    // The concurrent pre-resolve verification of the existing
    // lockfile must have its verdict before anything sensitive: the
    // symlink / bin-link phases, the dependency builds, and the
    // lockfile save below all run on a trusted lockfile only.
    await_lockfile_gate(lockfile_verification_gate).await?;
    fold_fetch_failures(skipped, std::mem::take(&mut materialized.fetch_failed));

    let linked = inputs.link::<Reporter>(&mut materialized, skipped)?;
    let crate::BuildModulesOutput {
        ignored_builds,
        deferred_builds,
        mutated_slots: _,
    } = inputs.build::<Reporter>(&materialized, &linked, skipped).await?;

    let injected_deps = crate::collect_injected_deps(
        ctx.linker.layout,
        ctx.workspace_root,
        lockfile.into(),
        skipped,
        ctx.is_hoisted().then_some(&linked.hoisted_locations),
    );
    Ok(OnDiskOutput {
        hoisted: pnpm_deps_restorer::InstalledHoistedState {
            dependencies: linked.hoisted_dependencies,
            locations: linked.hoisted_locations,
            injected_deps,
        },

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
    let materialized = materializer.finish(
        |key| wanted.is_some_and(|snapshots| snapshots.contains_key(key)) && !skipped.contains(key),
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

impl OnDiskProjects<'_> {
    fn project_manifests(
        &self,
        workspace_root: &Path,
        only_anchors: bool,
    ) -> Vec<(std::path::PathBuf, &PackageManifest)> {
        self.importer_manifests
            .iter()
            .filter(|(id, _)| {
                !only_anchors || self.project_anchor_importer_ids.contains(id.as_str())
            })
            .map(|(id, manifest)| (workspace_root.join(id), *manifest))
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
}

impl<'a> super::FreshPriorInstall<'a> {
    fn link_state(&self) -> pnpm_deps_restorer::PriorLinkState<'a> {
        pnpm_deps_restorer::PriorLinkState {
            prune_orphans: self.prune_orphans,
            hoisted_dependencies: self.hoisted_dependencies,
            hoisted_locations: self.hoisted_locations,
            // Rebuilds take the frozen path; a policy change rebuilds present packages here.
            build_present_packages: self.allow_builds_changed,
            unbuilt_builds: self.unbuilt_builds,
            previously_skipped: self.previously_skipped,
        }
    }
}
