use super::{FreshInputs, errors::InstallWithFreshLockfileError};
use crate::{AllowBuildPolicy, SkippedSnapshots, VirtualStoreLayout};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_package_manifest::PackageManifest;
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
    /// `None` when the built lockfile already is the closure: every
    /// importer materializes and the resolve pass walked no group this
    /// run leaves out.
    importer_ids: Option<HashSet<String>>,
    closure: Option<crate::MaterializationClosure>,
    /// The built lockfile's peer classification, shared by every walk of
    /// this install.
    pub(super) groups: crate::GroupSelection,
}
/// The second closure, with the importers that anchor project links.
pub(super) struct FinalScope {
    closure: Option<crate::MaterializationClosure>,
    pub(super) project_anchor_importer_ids: HashSet<String>,
}
impl MaterializationScope {
    pub(super) fn initial(install: FreshInputs<'_>, is_hoisted: bool, built: &Lockfile) -> Self {
        let importer_ids = closure_importer_ids(install, is_hoisted, built);
        let groups = crate::GroupSelection::classify(
            built,
            install.included(),
            install.drivers.config.peer_edge_options(),
        );
        let hoisted_prior = install.prior.lockfile.filter(|_| is_hoisted);
        let empty_skipped = SkippedSnapshots::new();
        let closure = match install.projects.selected_ids {
            Some(selected) => Some(crate::selected_materialization_closure(
                built,
                install.projects.lockfile_dir,
                selected,
                hoisted_prior,
                &groups,
                &empty_skipped,
            )),
            None => importer_ids
                .as_ref()
                .map(|importer_ids| {
                    crate::materialization_closure(
                        built,
                        install.projects.lockfile_dir,
                        importer_ids,
                        &groups,
                        &empty_skipped,
                    )
                }),
        };
        MaterializationScope { importer_ids, closure, groups }
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
        let hoisted_prior = install.prior.lockfile.filter(|_| is_hoisted);
        let closure = match install.projects.selected_ids {
            Some(selected) => Some(crate::selected_materialization_closure(
                built,
                install.projects.lockfile_dir,
                selected,
                hoisted_prior,
                &self.groups,
                skipped,
            )),
            None => self.importer_ids
                .as_ref()
                .map(|importer_ids| {
                    crate::materialization_closure(
                        built,
                        install.projects.lockfile_dir,
                        importer_ids,
                        &self.groups,
                        skipped,
                    )
                }),
        };
        let materialized: HashSet<String> = closure
            .as_ref()
            .map_or_else(
                || {
                    built.importers
                        .keys()
                        .cloned()
                        .collect()
                },
                |closure| closure.importer_ids.clone(),
            );
        let project_anchor_importer_ids =
            project_anchor_importer_ids(install.projects.selected_ids, is_hoisted, &materialized);
        FinalScope { closure, project_anchor_importer_ids }
    }
}
impl FinalScope {
    pub(super) fn lockfile<'l>(&'l self, built: &'l Lockfile) -> &'l Lockfile {
        self.closure.as_ref().map_or(built, |closure| &closure.lockfile)
    }

    pub(super) fn on_disk_projects<'l>(
        &'l self,
        built: &'l Lockfile,
        importer_manifests: &'l BTreeMap<String, &'l PackageManifest>,
    ) -> super::on_disk::OnDiskProjects<'l> {
        super::on_disk::OnDiskProjects {
            materialization_lockfile: self.lockfile(built),
            importer_manifests,
            project_anchor_importer_ids: &self.project_anchor_importer_ids,
        }
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
    scope: PlanScope<'_>,
) -> Result<FreshPlan<'l>, InstallWithFreshLockfileError> {
    let needs_deferred_probe =
        pnpm_deps_restorer::materialization_plan::needs_deferred_engine_probe(
            install.drivers.config,
            install.execution.node_linker,
            allow_build_policy,
            lockfiles.initial.snapshots.as_ref(),
            scope.known_requires_build,
        );
    let (installability_host, engine_name, deferred_engine_name) =
        resolve_host_and_engine(install, probe, lockfiles.initial, needs_deferred_probe).await;
    let (layout, dir_clone_cache) = verified_fresh_layout(
        install,
        lockfiles,
        allow_build_policy,
        engine_name.clone(),
        deferred_engine_name.as_ref(),
    )?;
    let installability_host = pnpm_deps_restorer::materialization_plan::with_locked_runtime_node(
        installability_host.as_ref(),
        install.drivers.config,
        &lockfiles.built.importers,
    );
    let host_node =
        installability_host.as_ref().map(pnpm_deps_restorer::materialization_plan::HostNode::from);
    let skipped = compute_fresh_skip_set::<Reporter>(
        install,
        lockfiles,
        installability_host.as_ref(),
        scope,
    )?;
    Ok(FreshPlan { host_node, engine_name, deferred_engine_name, layout, dir_clone_cache, skipped })
}

async fn resolve_host_and_engine(
    install: FreshInputs<'_>,
    probe: HostProbeInputs,
    initial: &Lockfile,
    needs_deferred_probe: bool,
) -> (
    Option<pnpm_deps_restorer::InstallabilityHost>,
    Option<String>,
    Option<pnpm_deps_restorer::materialization_plan::DeferredEngineName>,
) {
    let installability_host = installability_host(
        install.drivers.config,
        initial,
        probe.early_host_detection,
        (probe.node_version, install.projects.supported_architectures),
    )
    .await;
    let host_node =
        installability_host.as_ref().map(pnpm_deps_restorer::materialization_plan::HostNode::from);
    let (engine_name, deferred_engine_name) =
        pnpm_deps_restorer::materialization_plan::resolve_engine_name(
            install.drivers.config.enable_global_virtual_store,
            &initial.importers,
            host_node.as_ref(),
            needs_deferred_probe,
        )
        .await;
    (installability_host, engine_name, deferred_engine_name)
}

fn verified_fresh_layout<'l>(
    install: FreshInputs<'l>,
    lockfiles: PlanLockfiles<'l>,
    allow_build_policy: &'l AllowBuildPolicy,
    engine_name: Option<String>,
    deferred_engine_name: Option<&pnpm_deps_restorer::materialization_plan::DeferredEngineName>,
) -> Result<
    (VirtualStoreLayout, Option<pnpm_deps_restorer::DirCloneCache<'l>>),
    InstallWithFreshLockfileError,
> {
    let (layout, dir_clone_cache) = lay_out_slots(
        install,
        lockfiles.initial,
        allow_build_policy,
        engine_name,
        deferred_engine_name,
    );
    pnpm_deps_restorer::validate_virtual_store_slot_containment(
        lockfiles.initial.snapshots.as_ref(),
        &layout,
    )
    .map_err(InstallWithFreshLockfileError::LockfileVerification)?;
    Ok((layout, dir_clone_cache))
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
        install.drivers.config,
        install.drivers.config.enable_global_virtual_store
            .then_some(engine_name.as_deref())
            .flatten(),
        initial.snapshots.as_ref(),
        initial.packages.as_ref(),
        Some(allow_build_policy),
        Some(install.projects.lockfile_dir),
    );
    let dir_clone_cache = pnpm_deps_restorer::DirCloneCache::build(
        install.drivers.config,
        install.execution.node_linker,
        engine_name_source(deferred_engine_name, engine_name),
        initial.snapshots.as_ref(),
        initial.packages.as_ref(),
        Some(allow_build_policy),
        Some(install.projects.lockfile_dir),
    );
    log_layout_phase(install.drivers.config, phase_start);
    (layout, dir_clone_cache)
}
pub(super) fn compute_fresh_skip_set<Reporter: self::Reporter + 'static>(
    install: FreshInputs<'_>,
    lockfiles: PlanLockfiles<'_>,
    installability_host: Option<&pnpm_deps_restorer::InstallabilityHost>,
    scope: PlanScope<'_>,
) -> Result<SkippedSnapshots, InstallWithFreshLockfileError> {
    let closure_importer_ids: HashSet<String> = lockfiles.initial.importers
        .keys()
        .cloned()
        .collect();
    let all_groups = crate::GroupSelection::all();
    let closure_groups =
        if install.projects.selected_ids.is_some() { &all_groups } else { scope.groups };
    pnpm_deps_restorer::materialization_plan::compute_skip_set::<Reporter>(
        pnpm_deps_restorer::materialization_plan::SkipSetInputs {
            closure: pnpm_deps_restorer::SkipSetClosure {
                lockfile: lockfiles.initial,
                root: install.projects.lockfile_dir,
                importer_ids: &closure_importer_ids,
                groups: closure_groups,
            },
            entries: pnpm_lockfile::LockfileEntries {
                snapshots: lockfiles.initial.snapshots.as_ref(),
                packages: lockfiles.initial.packages.as_ref(),
            },
            requester: install.projects.requester,
            importers: &lockfiles.initial.importers,

            installability_host,
            // The fresh path has just re-resolved the graph, so the
            // previous run's verdicts may no longer hold.
            seed: SkippedSnapshots::new(),
            // Only an unfiltered full install's `dependency_groups` needs
            // `compute_skip_set` to drop `optional: true` snapshots: a
            // partial run keeps transitive optionals, and a `--filter`
            // install's `lockfiles.initial` has already stripped optional
            // dependencies from the selected importers while retaining
            // prior optional dependencies on unselected hoisted projects.
            exclude_optional: install.projects.selected_ids.is_none()
                && !scope.include_transitive_optional_dependencies,
            skip_runtimes: install.execution.skip_runtimes,
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
pub(super) struct PlanScope<'g> {
    pub(super) groups: &'g crate::GroupSelection,
    pub(super) include_transitive_optional_dependencies: bool,
    pub(super) known_requires_build: Option<&'g pnpm_deps_restorer::RequiresBuildBySnapshot>,
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
    included: IncludedDependencies,
) -> bool {
    !is_full_install || included.optional_dependencies
}
/// The importers the materialization closure walks, or `None` when the
/// built lockfile needs no narrowing.
///
/// Two things narrow it. A `--filter` selection picks the importers; a
/// `--prod` / `--dev` run picks their dependency groups. The resolve
/// pass walks every group to keep `pnpm-lock.yaml` complete, so this
/// closure is where the graph comes back down to what the run installs
/// (pnpm/pnpm#14912).
fn closure_importer_ids(
    install: FreshInputs<'_>,
    is_hoisted: bool,
    built: &Lockfile,
) -> Option<HashSet<String>> {
    let hoisted_prior = install.prior.lockfile.filter(|_| is_hoisted);
    materialization_importer_ids(install.projects.selected_ids, hoisted_prior, built)
        .or_else(|| {
            (install.resolve_widened_groups() || install.included().excludes_a_group()).then(|| {
                built.importers
                    .keys()
                    .cloned()
                    .collect()
            })
        })
}
/// The importers a selected install materializes. See
/// [`crate::selected_materialization_ids`] for `hoisted_prior`.
pub(super) fn materialization_importer_ids(
    selected_importer_ids: Option<&HashSet<String>>,
    hoisted_prior: Option<&Lockfile>,
    built_lockfile: &Lockfile,
) -> Option<HashSet<String>> {
    let selected_importer_ids = selected_importer_ids?;
    Some(crate::selected_materialization_ids(built_lockfile, selected_importer_ids, hoisted_prior))
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
    let needed = !config.installs_incompatible_packages()
        && lockfile.packages
            .as_ref()
            .zip(lockfile.snapshots.as_ref())
            .is_some_and(|(p, s)| crate::any_installability_constraint(s, p));
    match (early_host_detection, needed) {
        (Some(detection), true) => detection.resolve().await,
        (_, needed) => {
            pnpm_deps_restorer::materialization_plan::detect_installability_host(
                needed,
                config.effective_engine_strict(),
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
    match deferred.and_then(pnpm_deps_restorer::materialization_plan::DeferredEngineName::shared) {
        Some(slot) => pnpm_deps_restorer::EngineNameSource::Pending(slot),
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
    _selected_importer_ids: Option<&HashSet<String>>,
    _is_hoisted: bool,
    materialization_importer_ids: &HashSet<String>,
) -> HashSet<String> {
    materialization_importer_ids.clone()
}
