use super::{FreshInputs, errors::InstallWithFreshLockfileError};
use crate::{AllowBuildPolicy, SkippedSnapshots, VirtualStoreLayout};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::Reporter;
use std::collections::{BTreeMap, HashSet};

/// Which importers a selected install materializes, and the lockfile
/// closed over them.
///
/// Built twice: first without a skip set, to give the host probe and the
/// layout the snapshots they plan against, then over the skip set the
/// plan computed. The plan borrows the first closure's lockfile, so the
/// second is a separate value rather than a mutation of the first.
pub(super) struct MaterializationScope {
    /// `None` when every importer is materialized: the built lockfile
    /// is the closure.
    importer_ids: Option<HashSet<String>>,
    closure: Option<crate::MaterializationClosure>,
}
/// The second closure, with the importers that anchor project links.
pub(super) struct FinalScope {
    closure: Option<crate::MaterializationClosure>,
    pub(super) project_anchor_importer_ids: HashSet<String>,
}
impl MaterializationScope {
    pub(super) fn initial(install: FreshInputs<'_>, is_hoisted: bool, built: &Lockfile) -> Self {
        let importer_ids =
            materialization_importer_ids(install.selected_importer_ids, is_hoisted, built);
        let closure = importer_ids.as_ref().map(|importer_ids| {
            crate::materialization_closure(
                built,
                install.lockfile_dir,
                importer_ids,
                install.included(),
                &SkippedSnapshots::new(),
            )
        });
        MaterializationScope { importer_ids, closure }
    }

    pub(super) fn lockfile<'l>(&'l self, built: &'l Lockfile) -> &'l Lockfile {
        self.closure.as_ref().map_or(built, |closure| &closure.lockfile)
    }

    pub(super) fn finalize(
        &self,
        install: FreshInputs<'_>,
        is_hoisted: bool,
        built: &Lockfile,
        skipped: &SkippedSnapshots,
    ) -> FinalScope {
        let closure = self.importer_ids.as_ref().map(|importer_ids| {
            crate::materialization_closure(
                built,
                install.lockfile_dir,
                importer_ids,
                install.included(),
                skipped,
            )
        });
        let materialized: HashSet<String> = closure.as_ref().map_or_else(
            || built.importers.keys().cloned().collect(),
            |closure| closure.importer_ids.clone(),
        );
        let project_anchor_importer_ids =
            project_anchor_importer_ids(install.selected_importer_ids, is_hoisted, &materialized);
        FinalScope { closure, project_anchor_importer_ids }
    }
}
impl FinalScope {
    pub(super) fn lockfile<'l>(&'l self, built: &'l Lockfile) -> &'l Lockfile {
        self.closure.as_ref().map_or(built, |closure| &closure.lockfile)
    }
}
/// The lockfiles the materialization plan reads: the one the selected
/// importers materialize, and the full one the skip set's closure walks.
#[derive(Clone, Copy)]
pub(super) struct PlanLockfiles<'l> {
    pub(super) initial: &'l Lockfile,
    pub(super) built: &'l Lockfile,
}
/// What the on-disk phases read as settled. The skip set is the one part
/// they still change, as fetch failures and linking fold into it.
pub(super) struct FreshPlan<'l> {
    pub(super) host_node: Option<pnpm_deps_restorer::materialization_plan::HostNode>,
    pub(super) engine_name: Option<String>,
    pub(super) deferred_engine_name:
        Option<pnpm_deps_restorer::materialization_plan::DeferredEngineName>,
    pub(super) layout: VirtualStoreLayout,
    /// Borrows the initial lockfile and the allow-builds policy `run` owns.
    pub(super) dir_clone_cache: Option<pnpm_deps_restorer::DirCloneCache<'l>>,
    pub(super) skipped: SkippedSnapshots,
}
/// Detect the host, settle the engine name, build the layout and the
/// directory-clone cache, and compute the skip set. Consumes the early
/// host detection and the node version off `owned`.
pub(super) async fn plan_fresh_materialization<'l, 'a: 'l, Reporter: self::Reporter + 'static>(
    install: FreshInputs<'a>,
    probe: HostProbeInputs,
    lockfiles: PlanLockfiles<'l>,
    allow_build_policy: &'l AllowBuildPolicy,
    scope: PlanScope,
) -> Result<FreshPlan<'l>, InstallWithFreshLockfileError> {
    let installability_host = installability_host(
        install.config,
        lockfiles.initial,
        probe.early_host_detection,
        (probe.node_version, install.supported_architectures),
    )
    .await;
    let host_node =
        installability_host.as_ref().map(pnpm_deps_restorer::materialization_plan::HostNode::from);
    let (engine_name, deferred_engine_name) =
        pnpm_deps_restorer::materialization_plan::resolve_engine_name(
            install.config.enable_global_virtual_store,
            lockfiles.initial.snapshots.as_ref(),
            host_node.as_ref(),
        )
        .await;
    let (layout, dir_clone_cache) = lay_out_slots(
        install,
        lockfiles.initial,
        allow_build_policy,
        engine_name.clone(),
        deferred_engine_name.as_ref(),
    );
    let skipped = compute_fresh_skip_set::<Reporter>(
        install,
        lockfiles,
        installability_host.as_ref(),
        scope,
    )?;
    Ok(FreshPlan { host_node, engine_name, deferred_engine_name, layout, dir_clone_cache, skipped })
}
/// Build the slot layout and the directory-clone cache over the lockfile
/// the selected importers materialize.
pub(super) fn lay_out_slots<'l>(
    install: FreshInputs<'l>,
    initial: &'l Lockfile,
    allow_build_policy: &'l AllowBuildPolicy,
    engine_name: Option<String>,
    deferred_engine_name: Option<&pnpm_deps_restorer::materialization_plan::DeferredEngineName>,
) -> (VirtualStoreLayout, Option<pnpm_deps_restorer::DirCloneCache<'l>>) {
    let phase_start = std::time::Instant::now();
    let layout = VirtualStoreLayout::new(
        install.config,
        install.config.enable_global_virtual_store.then_some(engine_name.as_deref()).flatten(),
        initial.snapshots.as_ref(),
        initial.packages.as_ref(),
        Some(allow_build_policy),
        Some(install.lockfile_dir),
    );
    let dir_clone_cache = pnpm_deps_restorer::DirCloneCache::build(
        install.config,
        install.node_linker,
        engine_name_source(deferred_engine_name, engine_name),
        initial.snapshots.as_ref(),
        initial.packages.as_ref(),
        Some(allow_build_policy),
        Some(install.lockfile_dir),
    );
    log_layout_phase(install.config, phase_start);
    (layout, dir_clone_cache)
}
pub(super) fn compute_fresh_skip_set<Reporter: self::Reporter + 'static>(
    install: FreshInputs<'_>,
    lockfiles: PlanLockfiles<'_>,
    installability_host: Option<&pnpm_deps_restorer::InstallabilityHost>,
    scope: PlanScope,
) -> Result<SkippedSnapshots, InstallWithFreshLockfileError> {
    let closure_importer_ids: std::collections::HashSet<String> =
        lockfiles.built.importers.keys().cloned().collect();
    pnpm_deps_restorer::materialization_plan::compute_skip_set::<Reporter>(
        pnpm_deps_restorer::materialization_plan::SkipSetInputs {
            requester: install.requester,
            importers: &lockfiles.initial.importers,
            snapshots: lockfiles.initial.snapshots.as_ref(),
            packages: lockfiles.initial.packages.as_ref(),
            installability_host,
            // The fresh path has just re-resolved the graph, so the
            // previous run's verdicts may no longer hold.
            seed: SkippedSnapshots::new(),
            // Only a full install's `dependency_groups` carries a
            // `--no-optional` intent: a partial run either passes
            // every direct group (`add`, `remove`, `update`) or
            // narrows them for its own reasons (`fetch --dev`,
            // `rebuild`) and must keep its transitive optionals.
            exclude_optional: !scope.include_transitive_optional_dependencies,
            skip_runtimes: install.skip_runtimes,
            closure_lockfile: lockfiles.built,
            closure_root: install.lockfile_dir,
            closure_importer_ids: &closure_importer_ids,
            included: scope.included,
        },
    )
    .map_err(InstallWithFreshLockfileError::Installability)
}
pub(super) struct HostProbeInputs {
    pub(super) early_host_detection:
        Option<pnpm_deps_restorer::materialization_plan::HostDetection>,
    pub(super) node_version: Option<String>,
}
/// Which dependency groups the plan materializes.
#[derive(Clone, Copy)]
pub(super) struct PlanScope {
    pub(super) included: IncludedDependencies,
    pub(super) include_transitive_optional_dependencies: bool,
}
/// The two borrowed views `run` derives from [`Resolved`](crate::install_with_fresh_lockfile::resolution::Resolved) once the
/// resolve phase returns.
#[derive(Clone, Copy)]
pub(super) struct LockfileViews<'v, 'a> {
    pub(super) importer_manifests: &'v BTreeMap<String, &'a PackageManifest>,
    pub(super) wanted_lockfile: Option<&'v Lockfile>,
    pub(super) catalogs: &'v Catalogs,
    pub(super) lockfile_specifier_manifests: Option<&'v BTreeMap<String, PackageManifest>>,
}
pub(super) fn is_partial_workspace_selection(
    real_importer_ids: Option<&std::collections::HashSet<String>>,
    selected_importer_ids: Option<&std::collections::HashSet<String>>,
) -> bool {
    matches!(
        (real_importer_ids, selected_importer_ids),
        (Some(real), Some(selected)) if real != selected,
    )
}
pub(super) fn include_transitive_optional_dependencies(
    is_full_install: bool,
    dependency_groups: &[DependencyGroup],
) -> bool {
    !is_full_install || dependency_groups.contains(&DependencyGroup::Optional)
}
/// The importers a selected install materializes. A hoisted linker shares one
/// tree, so it still materializes every importer.
pub(super) fn materialization_importer_ids(
    selected_importer_ids: Option<&HashSet<String>>,
    is_hoisted: bool,
    built_lockfile: &Lockfile,
) -> Option<HashSet<String>> {
    let selected_importer_ids = selected_importer_ids?;
    if is_hoisted {
        return Some(built_lockfile.importers.keys().cloned().collect());
    }
    Some(selected_importer_ids.clone())
}
/// The host the installability checks run against, resolved from the
/// overlapped probe when one was started.
pub(super) async fn installability_host(
    config: &Config,
    lockfile: &Lockfile,
    early_host_detection: Option<pnpm_deps_restorer::materialization_plan::HostDetection>,
    host: (Option<String>, Option<&pnpm_package_is_installable::SupportedArchitectures>),
) -> Option<pnpm_deps_restorer::InstallabilityHost> {
    let (node_version, supported_architectures) = host;
    let needed = !config.force
        && lockfile.packages.as_ref().is_some_and(|packages| {
            lockfile
                .snapshots
                .as_ref()
                .is_some_and(|snapshots| crate::any_installability_constraint(snapshots, packages))
        });
    match (early_host_detection, needed) {
        (Some(detection), true) => detection.resolve().await,
        (_, needed) => {
            pnpm_deps_restorer::materialization_plan::detect_installability_host(
                needed,
                config.engine_strict,
                node_version,
                supported_architectures,
            )
            .await
        }
    }
}
pub(super) fn engine_name_source(
    deferred: Option<&pnpm_deps_restorer::materialization_plan::DeferredEngineName>,
    engine_name: Option<String>,
) -> pnpm_deps_restorer::EngineNameSource {
    match deferred {
        Some(deferred) => pnpm_deps_restorer::EngineNameSource::Pending(deferred.shared()),
        None => pnpm_deps_restorer::EngineNameSource::Ready(engine_name),
    }
}
/// Only a global virtual store makes the layout build cost anything worth
/// timing.
pub(super) fn log_layout_phase(config: &Config, phase_start: std::time::Instant) {
    if !config.enable_global_virtual_store {
        return;
    }
    tracing::info!(
        target: "pacquet::install::phase",
        phase = "virtual_store_layout_new",
        elapsed_ms = phase_start.elapsed().as_millis() as u64,
        "phase complete",
    );
}
/// The importers whose own project manifests anchor the link phase.
pub(super) fn project_anchor_importer_ids(
    selected_importer_ids: Option<&HashSet<String>>,
    is_hoisted: bool,
    materialization_importer_ids: &HashSet<String>,
) -> HashSet<String> {
    match selected_importer_ids {
        Some(selected_importer_ids) if is_hoisted => selected_importer_ids.clone(),
        _ => materialization_importer_ids.clone(),
    }
}
