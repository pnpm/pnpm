use super::{
    BuildInputs, ConcurrentVerification, FetchInputs, FrozenInputs, HostDetectionInputs, HostPlan,
    InstallFrozenLockfile, InstallFrozenLockfileError, LinkInputs, MaterializationPlan,
    PlanContextLayout, SkipSetPlan, build_extra_env,
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
            if install.drivers.config.package_provider.is_some() {
                super::link_provider_top_level_bins::<Reporter>(
                    &install,
                    phase.linked,
                    phase.skipped,
                )?;
                return Ok(crate::BuildModulesOutput {
                    ignored_builds: Vec::new(),
                    deferred_builds: Vec::new(),
                    mutated_slots: false,
                });
            }
            run_frozen_build_phase::<Reporter>(&install, ctx, phase).await
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
            plan_materialization_inner(
                &install,
                allow_build_policy,
                early_host_detection,
                node_version,
            )
            .await
        }
    }
}

async fn plan_materialization_inner<'p>(
    install: &FrozenInputs<'p>,
    allow_build_policy: &'p AllowBuildPolicy,
    early_host_detection: Option<crate::materialization_plan::HostDetection>,
    node_version: Option<String>,
) -> Result<MaterializationPlan<'p>, InstallFrozenLockfileError> {
    let entries = install.entries();
    let config = install.drivers.config;
    let needs_installability_check =
        needs_installability_check(config, entries.snapshots, entries.packages);
    let host_detection = detect_host(HostDetectionInputs {
        config,
        early_host_detection,
        node_version,
        supported_architectures: install.platform.supported_architectures,
        needs_installability_check,
    })
    .await;

    let engine = plan_engine_name(config, &host_detection, install.importers()).await;
    let layout = install.verified_layout(allow_build_policy, engine.name.as_deref())?;
    let dir_clone_cache = install.dir_clone_cache(allow_build_policy, engine.source());
    let cas_prefetch = crate::create_virtual_store::CasPrefetch::start(
        config,
        entries,
        allow_build_policy,
        install.platform.supported_architectures,
        None,
    )
    .await;

    Ok(MaterializationPlan {
        context_layout: PlanContextLayout {
            link_options: crate::shim_link_options(config, install.platform.node_linker),
            layout,
            dir_clone_cache,
            git_source_cache: pnpm_git_fetcher::GitSourceCache::default(),
        },
        host: HostPlan {
            host_detection,
            engine_name: engine.name,
            pending_host_engine_slot: engine.pending_slot,
            needs_installability_check,
        },
        deferred_engine_name: engine.deferred,
        cas_prefetch,
    })
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

async fn run_frozen_build_phase<'p, Reporter: self::Reporter>(
    install: &FrozenInputs<'p>,
    ctx: &'p crate::InstallContext<'p>,
    phase: BuildInputs<'p>,
) -> Result<crate::BuildModulesOutput, InstallFrozenLockfileError> {
    let LockfileEntries { packages, snapshots } = install.entries();
    let engine_name = match phase.deferred_engine_name {
        Some(deferred) => deferred.handle.await.ok().flatten(),
        None => phase.engine_name,
    };
    let build_extra_env = build_extra_env(
        install.drivers.config,
        install.platform.node_linker,
        install.projects.workspace_root,
    );
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
        extra_env: &build_extra_env,
        skipped: phase.skipped,
        held_back_bins_dirs: &phase.linked.held_back_bins_dirs,
    })
    .map_err(InstallFrozenLockfileError::BuildPhase)
}

pub(super) async fn fetch_frozen_store<'p, Reporter: self::Reporter>(
    install: &FrozenInputs<'p>,
    ctx: &'p crate::InstallContext<'p>,
    phase: FetchInputs<'p>,
) -> Result<CreateVirtualStoreOutput, InstallFrozenLockfileError> {
    let progress_reported = SharedReportedProgressKeys::default();
    let custom_fetcher_session = load_custom_fetcher_session(install.drivers.pnpmfile_hook).await?;
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
