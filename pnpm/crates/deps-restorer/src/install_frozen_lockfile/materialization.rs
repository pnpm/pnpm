use super::{
    BuildInputs, ConcurrentVerification, FetchInputs, HostDetectionInputs, HostPlan,
    InstallFrozenLockfile, InstallFrozenLockfileError, LinkInputs, MaterializationPlan,
    SkipSetPlan, build_extra_env,
    build_phase::{BuildPhaseInputs, run_build_phase},
    detect_host, fetch_verified, load_custom_fetcher_session, needs_installability_check,
    plan_engine_name, seed_skip_set, settle_engine_name,
};
use crate::{AllowBuildPolicy, CreateVirtualStoreOutput, SkippedSnapshots};
use pnpm_lockfile::LockfileEntries;
use pnpm_reporter::Reporter;
use pnpm_tarball::SharedReportedProgressKeys;
use std::future::Future;

impl<'a> InstallFrozenLockfile<'a> {
    /// Run the dependency builds, report ignored ones, and relink the
    /// top-level bins — the same build phase the fresh path runs.
    pub(super) fn build<'p, Reporter: self::Reporter>(
        &self,
        ctx: &'p crate::InstallContext<'p>,
        phase: BuildInputs<'p>,
    ) -> impl Future<Output = Result<crate::BuildModulesOutput, InstallFrozenLockfileError>>
    + Send
    + use<'p, 'a, Reporter> {
        let install = self.inputs();
        async move {
            let LockfileEntries { packages, snapshots } = install.entries();
            // Resolve the deferred `node --version` detection from the
            // GVS-off path, if any. The handle was spawned before
            // `CreateVirtualStore::run` so the `node` startup cost
            // overlapped with install I/O. Falls back to the synchronous
            // value when the spawn was never deferred (GVS on, or host
            // already detected for the installability check).
            let engine_name = match phase.deferred_engine_name {
                Some(deferred) => deferred.handle.await.ok().flatten(),
                None => phase.engine_name,
            };

            let build_extra_env = build_extra_env(
                install.drivers.config,
                install.platform.node_linker,
                install.projects.workspace_root,
            );

            // Run lifecycle scripts, report ignored builds, and re-link
            // top-level bins. `workspace_root` is the `lockfileDir`;
            // pass the real `Path` rather than reconstructing it from the
            // lossy `requester` string so non-UTF-8 filenames survive.
            // `allow_build_policy` was constructed up-front (before
            // `CreateVirtualStore`) so the git fetcher could consult it.
            run_build_phase::<Reporter>(&BuildPhaseInputs {
                cache: phase.fetched.build_cache(engine_name.as_deref(), phase.store_index_writer),
                directories: build_directories(install.projects.workspace_root, ctx, phase.linked),
                graph: crate::BuildPhaseGraph {
                    snapshots,
                    packages,
                    importers: &install.lockfiles.wanted.importers,
                    dependency_groups: install.projects.dependency_groups,
                    materialized_snapshots: phase.linked.build_snapshots(
                        &phase.fetched.materialized_snapshots,
                    ),
                },
                policy: install.build_policy(ctx.allow_build_policy),

                // Resolved once inside `resolve_snapshot_patches`; the frozen
                // path has no earlier patch resolution to reuse.
                extra_env: &build_extra_env,

                skipped: phase.skipped,
                held_back_bins_dirs: &phase.linked.held_back_bins_dirs,
            })
            .map_err(InstallFrozenLockfileError::BuildPhase)
        }
    }
    /// Link the materialized store into every project: the direct
    /// dependencies, the hoisted tree, and the sidecars.
    pub(super) fn link<Reporter: self::Reporter>(
        &self,
        ctx: &crate::InstallContext<'_>,
        phase: LinkInputs<'_>,
        skipped: &mut SkippedSnapshots,
    ) -> Result<crate::linking::LinkPhaseOutput, InstallFrozenLockfileError> {
        let install = self.inputs();
        let (trusted_importer_ids, root_component_importers) = install.importer_sets();
        crate::linking::run_link_phase::<Reporter>(
            crate::linking::LinkPhaseInputs {
                graph: crate::LinkLockfiles {
                    lockfile: install.lockfiles.wanted,
                    current_lockfile: install.lockfiles.current,
                    materialized_snapshots: (install.prior.rebuild.is_none()
                        && !install.prior.relink_every_slot_bin)
                        .then_some(phase.fetched.materialized_snapshots.as_slice()),
                    sidecar_lockfile: phase.current_lockfile,
                },
                packages: crate::LinkPackageData {
                    package_manifests: &phase.fetched.package_manifests,
                    requires_build_by_snapshot: Some(&phase.fetched.requires_build_by_snapshot),
                    cas_paths_by_pkg_id: phase.cas_paths_by_pkg_id,
                },
                prior: install.prior.link_state(),
                projects: crate::LinkProjects {
                    manifests: install.projects.manifests,
                    package_map_manifests: install.projects.package_map_manifests,
                    dependency_groups: install.projects.dependency_groups,
                    symlink_root: install.projects.workspace_root,
                    trusted_importer_ids: &trusted_importer_ids,
                    root_component_importers: &root_component_importers,
                },
                ctx,

                host_node: phase.host_node,
                supported_architectures: install.platform.supported_architectures,
            },
            skipped,
        )
        .map_err(InstallFrozenLockfileError::LinkPhase)
    }
    /// Materialize the virtual store under concurrent lockfile
    /// verification. See [`fetch_verified`] for the ordering rule.
    pub(super) fn fetch<'p, Reporter: self::Reporter>(
        &self,
        ctx: &'p crate::InstallContext<'p>,
        phase: FetchInputs<'p>,
    ) -> impl Future<Output = Result<CreateVirtualStoreOutput, InstallFrozenLockfileError>>
    + Send
    + use<'p, 'a, Reporter>
    where
        'a: 'p,
    {
        let install = self.inputs();
        async move {
            // The frozen path runs no resolve-time prefetcher, so the warm
            // batch owns package-status progress for store hits. An empty set
            // leaves every warm package reported as `found_in_store`.
            let progress_reported = SharedReportedProgressKeys::default();

            let custom_fetcher_session =
                load_custom_fetcher_session(install.drivers.pnpmfile_hook).await?;
            // Timed from here: a pnpmfile's fetcher setup is hook work, not
            // materialization, and the integrated benchmark reads this phase.
            let phase_start = std::time::Instant::now();
            let output = fetch_verified::<Reporter>(
                install.virtual_store(
                    ctx,
                    (phase.cas_prefetch, phase.dir_clone_cache),
                    phase.store_index_writer,
                    phase.skipped,
                    &progress_reported,
                    custom_fetcher_session.as_ref(),
                ),
                ConcurrentVerification {
                    lockfile: install.lockfiles.verified,
                    verifiers: install.lockfiles.resolution_verifiers,
                    precomputed: phase.verification_override,
                    lockfile_path: install.lockfiles.path,
                    cache_dir: &install.drivers.config.cache_dir,
                },
            )
            .await?;
            tracing::info!(
                target: "pacquet::install::phase",
                phase = "create_virtual_store",
                elapsed_ms = phase_start.elapsed().as_millis() as u64,
                "phase complete",
            );
            Ok(output)
        }
    }
    /// Resolve the host probe the plan left pending, settle the engine
    /// name it decides, and compute which snapshots this host installs.
    pub(super) fn settle_skip_set<Reporter: self::Reporter>(
        &self,
        host: HostPlan,
        seed_skipped: Option<Vec<String>>,
    ) -> impl Future<Output = Result<SkipSetPlan, InstallFrozenLockfileError>> + Send {
        let inputs = self.inputs();
        async move {
            let phase_start = std::time::Instant::now();
            let installability_host = host.host_detection.resolve().await;
            report_host_detection_wait(phase_start, host.needs_installability_check);
            let host_node =
                installability_host.as_ref().map(crate::materialization_plan::HostNode::from);
            // Deliver the host-derived engine name to the directory-clone
            // cache's shared slot before anything can wait on it, and pick
            // it up for `BuildModules` below.
            let engine_name = settle_engine_name(
                host.pending_host_engine_slot.as_deref(),
                host.engine_name,
                host_node.as_ref(),
            );
            let installability_host = crate::materialization_plan::with_locked_runtime_node(
                installability_host.as_ref(),
                inputs.drivers.config,
                &inputs.lockfiles.wanted.importers,
            );
            let skipped = compute_frozen_skip_set::<Reporter>(
                &inputs,
                installability_host.as_ref(),
                seed_skipped,
            )?;
            let host_node =
                installability_host.as_ref().map(crate::materialization_plan::HostNode::from);
            Ok(SkipSetPlan { skipped, engine_name, host_node })
        }
    }
    /// Everything the on-disk phases need decided before any of them
    /// starts: the policies, the slot layout the lockfile validates
    /// against, the engine name the layout and the build cache key on,
    /// and the store-side prefetch that runs while the host probe
    /// finishes.
    ///
    /// Not an `async fn`: the returned future must be `Send`, and a
    /// future holding `&self` is not, because the verification override
    /// is a boxed future without `Sync`. Only the `Copy` inputs are
    /// captured.
    pub(super) fn plan_materialization<'p>(
        &self,
        allow_build_policy: &'p AllowBuildPolicy,
        early_host_detection: Option<crate::materialization_plan::HostDetection>,
        node_version: Option<String>,
    ) -> impl Future<Output = Result<MaterializationPlan<'p>, InstallFrozenLockfileError>> + Send
    where
        'a: 'p,
    {
        let install = self.inputs();
        async move {
            let LockfileEntries { packages, snapshots } = install.entries();
            let config = install.drivers.config;

            // TODO: check if the lockfile is out-of-date

            let needs_installability_check =
                needs_installability_check(config, snapshots, packages);

            // The host detection is what costs a `node --version` probe
            // (~150 ms of node startup). The global-virtual-store layout
            // needs the engine name — and so the host — synchronously
            // below, but otherwise the detection stays pending and is only
            // resolved once the skip-set computation needs the host, so
            // the probe runs under the store-side warm-cache prefetch
            // instead of serializing before it. A detection the install
            // entry point already spawned (before the lockfile parse) is
            // adopted so its head start counts; an early detection a
            // constraint-free lockfile turns out not to need is dropped —
            // the probe finishes in the background and its result goes
            // unused.
            let host_detection = detect_host(HostDetectionInputs {
                config,
                early_host_detection,
                node_version,
                supported_architectures: install.platform.supported_architectures,
                needs_installability_check,
            })
            .await;

            // `engine_name` feeds two sites:
            //
            // - The GVS-aware `VirtualStoreLayout` needs it *before*
            //   `CreateVirtualStore::run` to produce per-snapshot
            //   `<scope>/<name>/<version>/<hash>` suffixes under
            //   `<store_dir>/links`. Only matters when GVS is on.
            // - `BuildModules` uses it for the side-effects-cache key
            //   prefix. Read by both the cache read-gate and the
            //   write-gate (see `build_modules.rs:346-350`); when
            //   `None`, both gates close and the cache is bypassed.
            //
            // Honour `engines.runtime` / `devEngines.runtime` pin (if
            // one reached the lockfile): the runtime resolver writes
            // the chosen Node as a `node@runtime:<version>` snapshot, and
            // the engine-name helper anchors the GVS hash and the
            // side-effects-cache key prefix to that pinned Node —
            // otherwise pacquet hashes under whatever
            // `node --version` returns from the shell, splitting the
            // shared store between pinned and non-pinned installs on the
            // same host.
            //
            // Four paths, the first that applies wins:
            // - Root project's runtime pin: the name is known outright.
            // - Host detection still pending (constraint-bearing lockfile,
            //   GVS off): the name is derived from the host once it
            //   resolves below; until then the directory-clone cache reads
            //   it through a shared slot from its lazily built layout.
            // - Host already detected (GVS on, or no check needed): reuse
            //   it synchronously. Synthetic-fallback (`node_detected =
            //   false`) yields `None` so a bogus `99999.0.0`-derived key
            //   can't poison either the cache or the GVS hash.
            // - No host at all: GVS spawns `node --version` synchronously
            //   (its layout needs the result); otherwise the probe is
            //   deferred into the blocking pool, overlaps
            //   `CreateVirtualStore::run`'s I/O, and is awaited right
            //   before `BuildModules`.
            let engine = plan_engine_name(config, &host_detection, install.importers()).await;

            let layout = install.verified_layout(allow_build_policy, engine.name.as_deref())?;

            // Built after the offline lockfile checks above: constructing
            // the cache probes the filesystem (a store-side write), which a
            // rejected lockfile must never reach.
            let dir_clone_cache = install.dir_clone_cache(allow_build_policy, engine.source());

            // Kick off the store-side half of `CreateVirtualStore::run`'s
            // planning — like the directory-clone cache above, only after
            // the offline lockfile checks — so its index reads run while a
            // pending host detection finishes its `node --version`.
            let cas_prefetch = crate::create_virtual_store::CasPrefetch::start(
                config,
                install.entries(),
                allow_build_policy,
                install.platform.supported_architectures,
                None,
            )
            .await;
            Ok(MaterializationPlan {
                link_options: crate::shim_link_options(config, install.platform.node_linker),
                host: HostPlan {
                    host_detection,
                    engine_name: engine.name,
                    pending_host_engine_slot: engine.pending_slot,
                    needs_installability_check,
                },
                deferred_engine_name: engine.deferred,
                layout,
                dir_clone_cache,
                cas_prefetch,
                git_source_cache: pnpm_git_fetcher::GitSourceCache::default(),
            })
        }
    }
}

fn build_directories<'a>(
    workspace_root: &'a std::path::Path,
    ctx: &'a crate::InstallContext<'a>,
    linked: &'a crate::linking::LinkPhaseOutput,
) -> crate::BuildPhaseDirectories<'a> {
    crate::BuildPhaseDirectories {
        workspace_root,
        top_level_bin_root: workspace_root,
        layout: ctx.linker.layout,
        hoisted_pkg_roots_by_key: linked.hoisted_pkg_roots_by_key.as_ref(),
        is_hoisted: ctx.is_hoisted(),
        publicly_hoisted_for_post_build: &linked.publicly_hoisted_for_post_build,
        logged_methods: ctx.logged_methods,
        link_options: ctx.linker.bin_options,
    }
}

fn compute_frozen_skip_set<Reporter: self::Reporter>(
    inputs: &super::FrozenInputs<'_>,
    installability_host: Option<&crate::InstallabilityHost>,
    seed_skipped: Option<Vec<String>>,
) -> Result<SkippedSnapshots, InstallFrozenLockfileError> {
    let groups = inputs.groups();
    crate::materialization_plan::compute_skip_set::<Reporter>(
        crate::materialization_plan::SkipSetInputs {
            closure: crate::SkipSetClosure {
                lockfile: inputs.lockfiles.wanted,
                root: inputs.projects.workspace_root,
                importer_ids: &inputs.lockfiles.wanted.importers
                    .keys()
                    .cloned()
                    .collect(),
                groups,
            },
            entries: inputs.entries(),
            requester: inputs.projects.requester,
            importers: &inputs.lockfiles.wanted.importers,

            installability_host,
            seed: seed_skip_set(inputs.drivers.config, seed_skipped),
            // The frozen path always installs the groups it was
            // given, so `--no-optional` needs no further
            // qualification here.
            exclude_optional: !groups.included.optional_dependencies,
            skip_runtimes: inputs.platform.skip_runtimes,
        },
    )
    .map_err(InstallFrozenLockfileError::Installability)
}

fn report_host_detection_wait(phase_start: std::time::Instant, needs_installability_check: bool) {
    if needs_installability_check {
        tracing::info!(
            target: "pacquet::install::phase",
            phase = "await_installability_host",
            elapsed_ms = phase_start.elapsed().as_millis() as u64,
            "phase complete",
        );
    }
}
