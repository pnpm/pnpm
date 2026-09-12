//! The install link phase, shared by both paths: reconcile what the previous install
//! left behind, materialize the importer-visible tree — either the
//! hoisted `node_modules` hierarchy or the isolated symlink layout with
//! its hoist and bin passes — and write the module-resolution sidecars.
//!
//! Runs once the virtual store is populated and before the build phase,
//! which needs both the linked tree and the hoisted package roots this
//! reports.

use crate::{
    CasPathsByPkgId, LinkVirtualStoreBins, PackageManifests, SkippedSnapshots,
    SymlinkDirectDependencies, VirtualStoreLayout,
    install_frozen_lockfile::{
        HoistPlan, HoistedLinkerError, HoistedLinkerInputs, collect_public_hoist_targets,
        compute_hoist_plan, run_hoisted_linker, workspace_packages_for_hoist,
    },
    link_direct_dep_bins_resolved, link_root_component_members, symlink_hoisted_dependencies,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_cmd_shim::LinkBinsOptions;
use pnpm_config::{Config, NodeLinker};
use pnpm_lockfile::{Lockfile, PackageKey};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{LogEvent, LogLevel, Reporter, StatsLog, StatsMessage};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

/// Error type of [`run_link_phase`].
///
/// The wrapping variants are `#[diagnostic(transparent)]`, so the
/// surfaced `ERR_PNPM_*` code is the inner error's and a link failure
/// reports identically whichever install path ran it. The two sidecar
/// writes are the exception: they own
/// `ERR_PNPM_PACKAGE_MANAGER_WRITE_PACKAGE_MAP` and
/// `ERR_PNPM_PACKAGE_MANAGER_WRITE_PNP_FILE` because the underlying I/O
/// error carries no pnpm code of its own.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum LinkPhaseError {
    #[diagnostic(transparent)]
    PruneStaleModules(#[error(source)] crate::PruneDirectDepsError),
    #[diagnostic(transparent)]
    SymlinkDirectDependencies(#[error(source)] crate::SymlinkDirectDependenciesError),
    #[diagnostic(transparent)]
    LinkRootComponentMembers(#[error(source)] crate::LinkRootComponentMembersError),
    #[diagnostic(transparent)]
    LinkVirtualStoreBins(#[error(source)] crate::LinkVirtualStoreBinsError),
    #[diagnostic(transparent)]
    HoistSymlink(#[error(source)] crate::SymlinkPackageError),
    #[diagnostic(transparent)]
    HoistLinkBins(#[error(source)] pnpm_cmd_shim::LinkBinsError),
    #[diagnostic(transparent)]
    LinkBins(#[error(source)] pnpm_cmd_shim::LinkBinsError),
    #[diagnostic(transparent)]
    HoistedDepGraph(#[error(source)] crate::HoistedDepGraphError),
    #[diagnostic(transparent)]
    LinkHoistedModules(#[error(source)] crate::LinkHoistedModulesError),
    #[display("failed to write package map: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_WRITE_PACKAGE_MAP))]
    WritePackageMap(#[error(source)] crate::WritePackageMapError),
    #[display("failed to write PnP loader: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_WRITE_PNP_FILE))]
    WritePnpFile(#[error(source)] crate::WritePnpFileError),
}

impl From<HoistedLinkerError> for LinkPhaseError {
    fn from(error: HoistedLinkerError) -> Self {
        match error {
            HoistedLinkerError::HoistedDepGraph(error) => LinkPhaseError::HoistedDepGraph(error),
            HoistedLinkerError::LinkHoistedModules(error) => {
                LinkPhaseError::LinkHoistedModules(error)
            }
            HoistedLinkerError::SymlinkDirectDependencies(error) => {
                LinkPhaseError::SymlinkDirectDependencies(error)
            }
            HoistedLinkerError::WritePackageMap(error) => LinkPhaseError::WritePackageMap(error),
        }
    }
}

/// Everything the link phase reads.
///
/// Both install paths supply this. What differs between them is carried
/// as a field value rather than a branch inside the phase; each such
/// field documents its per-path value.
pub struct LinkPhaseInputs<'a> {
    pub ctx: &'a crate::InstallContext<'a>,
    /// The lockfile this phase links. Its `importers`, `packages` and
    /// `snapshots` are read straight off it, so a caller cannot pair one
    /// lockfile's entries with another's maps.
    pub lockfile: &'a Lockfile,
    pub current_lockfile: Option<&'a Lockfile>,
    /// Restricts per-slot bin linking to this install's materialized
    /// snapshots. `None` keeps rebuild's all-slot behavior.
    pub materialized_snapshots: Option<&'a [PackageKey]>,
    pub project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub package_map_project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub dependency_groups: &'a [DependencyGroup],
    pub package_manifests: &'a PackageManifests,
    /// Per-snapshot `requiresBuild` flags from the store-index prefetch,
    /// gating the importer bin pass's use of `package_manifests` — see
    /// [`crate::link_direct_dep_bins_prefetched`]. `None` when the
    /// caller has no prefetch (the fresh-lockfile installer).
    pub requires_build_by_snapshot: Option<&'a crate::RequiresBuildBySnapshot>,
    pub cas_paths_by_pkg_id: Option<CasPathsByPkgId>,
    /// Anchor for each importer's `node_modules`. The frozen path uses
    /// `workspace_root`; the fresh path uses `modules_dir.parent()`,
    /// because its tests relocate `modules_dir` away from the manifest.
    pub symlink_root: &'a Path,
    /// Importer ids allowed to live outside the lockfile dir (Bit's
    /// capsule installs).
    pub trusted_importer_ids: &'a std::collections::HashSet<String>,
    /// Importers declaring `installConfig.hoistingLimits: "workspaces"`.
    pub root_component_importers: &'a std::collections::HashSet<String>,
    /// The lockfile the module-resolution sidecars describe. The frozen
    /// path filters to the current install first; the fresh path already
    /// holds a materialization closure.
    pub sidecar_lockfile: &'a Lockfile,
    pub prune_orphans: bool,
    pub prior_hoisted_dependencies: Option<&'a crate::HoistedDependencies>,
    /// `hoistedLocations` recorded by the previous install's
    /// `.modules.yaml`; lets the hoisted linker leave packages that are
    /// already in place alone. `None` on a first install.
    pub prior_hoisted_locations: Option<&'a crate::HoistedLocations>,
    /// See [`crate::HoistedLinkerInputs::build_present_packages`].
    pub build_present_packages: bool,
    /// See [`crate::HoistedLinkerInputs::prior_unbuilt_builds`].
    pub prior_unbuilt_builds: &'a crate::UnbuiltBuilds,
    pub host_node: Option<&'a crate::materialization_plan::HostNode>,
    pub supported_architectures: Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
}

/// What the link phase hands to the build phase and the caller's
/// `.modules.yaml` writer.
pub struct LinkPhaseOutput {
    pub hoisted_dependencies: crate::HoistedDependencies,
    pub hoisted_locations: BTreeMap<String, Vec<String>>,
    pub hoisted_pkg_roots_by_key: Option<HashMap<PackageKey, Vec<PathBuf>>>,
    /// See [`crate::HoistedLinkerOutput::hoisted_build_snapshots`].
    pub hoisted_build_snapshots: Option<Vec<PackageKey>>,
    /// Publicly-hoisted aliases carrying bins. Public hoist promotes a
    /// transitive dep to `<root>/node_modules/<alias>`, whose bin then
    /// competes for the same `<root>/node_modules/.bin` slot as a root
    /// direct dep's; per pnpm/pacquet#342 the direct dep must win. The
    /// post-`BuildModules` top-level bin link takes both candidate lists
    /// so `pick_winner`'s [`BinOrigin`] tier settles it in one call.
    ///
    /// [`BinOrigin`]: pnpm_cmd_shim::BinOrigin
    pub publicly_hoisted_for_post_build: Vec<String>,
}

impl LinkPhaseOutput {
    /// Hoisted builds honor the linker's presence and build-policy decisions; other linkers
    /// use the virtual store's materialized snapshots.
    #[must_use]
    pub fn build_snapshots<'a>(&'a self, materialized: &'a [PackageKey]) -> &'a [PackageKey] {
        self.hoisted_build_snapshots.as_deref().unwrap_or(materialized)
    }

    /// The result of a run that materialized nothing.
    fn empty() -> Self {
        LinkPhaseOutput {
            hoisted_dependencies: crate::HoistedDependencies::new(),
            hoisted_locations: BTreeMap::new(),
            hoisted_pkg_roots_by_key: None,
            hoisted_build_snapshots: None,
            publicly_hoisted_for_post_build: Vec::new(),
        }
    }
}

/// What [`write_hoist_links`] put on disk.
struct HoistLinks {
    hoisted_dependencies: crate::HoistedDependencies,
    /// See [`LinkPhaseOutput::publicly_hoisted_for_post_build`].
    publicly_hoisted_with_bins: Vec<String>,
}

impl HoistLinks {
    /// The result of a run with no hoist plan to write.
    fn none() -> Self {
        HoistLinks {
            hoisted_dependencies: crate::HoistedDependencies::new(),
            publicly_hoisted_with_bins: Vec::new(),
        }
    }
}

/// Reconcile what the previous install left behind, then materialize
/// the importer-visible tree and the module-resolution sidecars.
///
/// **Precondition:** the virtual store is already populated. This
/// creates links into it and never fetches, so a snapshot missing from
/// the store yields a dangling link rather than an error.
///
/// `skipped` is taken by `&mut` because the hoisted linker adds to it:
/// a package the walker cannot place is recorded so the build phase and
/// `.modules.yaml` observe the same skip set this phase acted on.
///
/// Returns what the build phase and the caller's `.modules.yaml` writer
/// need — see [`LinkPhaseOutput`]. Under `virtual_store_only` only the
/// per-slot bin pass runs and every output is empty: that mode
/// populates the virtual store without touching the project.
pub fn run_link_phase<Reporter: self::Reporter>(
    inputs: LinkPhaseInputs<'_>,
    skipped: &mut SkippedSnapshots,
) -> Result<LinkPhaseOutput, LinkPhaseError> {
    let hoist = plan_hoist(&inputs, skipped);

    // `nodeLinker: hoisted` writes no virtual store — `CreateVirtualStore`
    // skipped the slots — so there is nothing to link into or out of.
    let has_virtual_store = !inputs.ctx.is_hoisted();
    if has_virtual_store && !inputs.ctx.config.virtual_store_only {
        relink_importer_tree::<Reporter>(&inputs, skipped, hoist.public_targets.as_ref())?;
    }
    if has_virtual_store {
        link_virtual_store_bins(&inputs, skipped)?;
    }

    // Everything below writes into the project that `virtual_store_only`
    // exists to leave alone.
    if inputs.ctx.config.virtual_store_only {
        return Ok(LinkPhaseOutput::empty());
    }
    write_project_links::<Reporter>(inputs, skipped, hoist.plan)
}

/// Planned before the links are written, not with them: an importer's
/// dep that public-hoist lands at root has to be in
/// `SymlinkDirectDependencies`'s dedupe map, and [`write_hoist_links`]
/// reuses the plan rather than walking a second time.
struct PlannedHoist {
    plan: Option<HoistPlan>,
    public_targets: Option<BTreeMap<String, PathBuf>>,
}

fn plan_hoist(inputs: &LinkPhaseInputs<'_>, skipped: &SkippedSnapshots) -> PlannedHoist {
    let config = inputs.ctx.config;
    // `hoistWorkspacePackages`: named non-root projects become hoist
    // candidates whose links point at the project dirs.
    let hoisted_workspace_packages = config
        .hoist_workspace_packages
        .then(|| workspace_packages_for_hoist(inputs.ctx.workspace_root, inputs.project_manifests));
    let phase_start = std::time::Instant::now();
    let plan = compute_hoist_plan(
        config,
        inputs.lockfile.snapshots.as_ref(),
        inputs.lockfile.packages.as_ref(),
        &inputs.lockfile.importers,
        inputs.dependency_groups,
        skipped,
        inputs.ctx.is_hoisted(),
        hoisted_workspace_packages.as_ref(),
    );
    let public_targets = plan.as_ref().map(|plan| {
        collect_public_hoist_targets(&plan.result, &plan.graph, inputs.ctx.layout, &plan.skipped)
    });
    tracing::info!(target: "pacquet::install::phase", phase = "link.hoist_plan", elapsed_ms = phase_start.elapsed().as_millis() as u64, "phase complete");
    PlannedHoist { plan, public_targets }
}

/// Reconcile first, so stale direct-dep and orphaned hoist links vacate
/// the slots the relink + rehoist claim. This `removed` pairs with the
/// `added` that `CreateVirtualStore` emits, keeping it one pair per
/// install. The hoisted linker reconciles and emits its own pair
/// instead (see [`crate::link_hoisted_modules()`]).
fn relink_importer_tree<Reporter: self::Reporter>(
    inputs: &LinkPhaseInputs<'_>,
    skipped: &SkippedSnapshots,
    public_hoist_targets: Option<&BTreeMap<String, PathBuf>>,
) -> Result<(), LinkPhaseError> {
    let config = inputs.ctx.config;
    prune_importer_tree::<Reporter>(inputs)?;

    let phase_start = std::time::Instant::now();
    SymlinkDirectDependencies {
        config,
        layout: inputs.ctx.layout,
        importers: &inputs.lockfile.importers,
        packages: inputs.lockfile.packages.as_ref(),
        dependency_groups: inputs.dependency_groups.iter().copied(),
        workspace_root: inputs.symlink_root,
        skipped,
        link_only: false,
        public_hoist_targets,
        trusted_importer_ids: Some(inputs.trusted_importer_ids),
        link_options: inputs.ctx.link_options,
        package_manifests: Some(inputs.package_manifests),
        requires_build_by_snapshot: inputs.requires_build_by_snapshot,
    }
    .run::<Reporter>()
    .map_err(LinkPhaseError::SymlinkDirectDependencies)?;
    tracing::info!(target: "pacquet::install::phase", phase = "link.symlink_direct_deps", elapsed_ms = phase_start.elapsed().as_millis() as u64, "phase complete");

    // Bit "root components" — a no-op unless an importer declared
    // `installConfig.hoistingLimits: "workspaces"`.
    link_root_component_members(
        inputs.ctx.layout,
        &inputs.lockfile.importers,
        inputs.lockfile.snapshots.as_ref(),
        inputs.root_component_importers,
        inputs.dependency_groups,
        skipped,
    )
    .map_err(LinkPhaseError::LinkRootComponentMembers)
}

fn prune_importer_tree<Reporter: self::Reporter>(
    inputs: &LinkPhaseInputs<'_>,
) -> Result<(), LinkPhaseError> {
    let config = inputs.ctx.config;
    let removed_count = inputs
        .current_lockfile
        .map(|current| {
            crate::PruneStaleModules {
                config,
                workspace_root: inputs.symlink_root,
                wanted_lockfile: inputs.lockfile,
                current_lockfile: current,
                prior_hoisted_dependencies: inputs.prior_hoisted_dependencies,
                included_groups: inputs.dependency_groups,
                prune_orphans: inputs.prune_orphans,
            }
            .run::<Reporter>()
            .map_err(LinkPhaseError::PruneStaleModules)
        })
        .transpose()?
        .unwrap_or(0);
    Reporter::emit(&LogEvent::Stats(StatsLog {
        level: LogLevel::Debug,
        message: StatsMessage::Removed {
            prefix: inputs.ctx.requester.to_owned(),
            removed: removed_count,
        },
    }));

    Ok(())
}

/// Unlike every other link pass this one also runs under
/// `virtual_store_only`: the links it writes live inside the virtual
/// store, and the build phase — which `pnpm fetch` still runs — resolves
/// a dependency's sibling bin through them.
fn link_virtual_store_bins(
    inputs: &LinkPhaseInputs<'_>,
    skipped: &SkippedSnapshots,
) -> Result<(), LinkPhaseError> {
    let phase_start = std::time::Instant::now();
    LinkVirtualStoreBins {
        layout: inputs.ctx.layout,
        snapshots: inputs.lockfile.snapshots.as_ref(),
        selected_snapshots: inputs.materialized_snapshots,
        packages: inputs.lockfile.packages.as_ref(),
        package_manifests: inputs.package_manifests,
        skipped,
        link_options: inputs.ctx.link_options,
    }
    .run()
    .map_err(LinkPhaseError::LinkVirtualStoreBins)?;
    tracing::info!(target: "pacquet::install::phase", phase = "link.virtual_store_bins", elapsed_ms = phase_start.elapsed().as_millis() as u64, "phase complete");
    Ok(())
}

fn write_project_links<Reporter: self::Reporter>(
    mut inputs: LinkPhaseInputs<'_>,
    skipped: &mut SkippedSnapshots,
    pre_hoist: Option<HoistPlan>,
) -> Result<LinkPhaseOutput, LinkPhaseError> {
    let config = inputs.ctx.config;
    let hoisted = link_hoisted_projects::<Reporter>(&mut inputs, skipped)?;

    let bin_deps = public_workspace_bin_deps(pre_hoist.as_ref());
    let phase_start = std::time::Instant::now();
    let links = pre_hoist
        .map(|plan| write_hoist_links(plan, config, inputs.ctx.layout, inputs.ctx.link_options))
        .transpose()?
        .unwrap_or_else(HoistLinks::none);
    tracing::info!(target: "pacquet::install::phase", phase = "link.write_hoist_links", elapsed_ms = phase_start.elapsed().as_millis() as u64, "phase complete");

    write_project_sidecars(&inputs)?;
    if !bin_deps.is_empty() {
        link_direct_dep_bins_resolved(&config.modules_dir, &bin_deps, inputs.ctx.link_options)
            .map_err(LinkPhaseError::LinkBins)?;
    }

    Ok(LinkPhaseOutput {
        hoisted_dependencies: links.hoisted_dependencies,
        hoisted_locations: hoisted.hoisted_locations,
        hoisted_pkg_roots_by_key: hoisted.hoisted_pkg_roots_by_key,
        hoisted_build_snapshots: hoisted.hoisted_build_snapshots,
        publicly_hoisted_for_post_build: links.publicly_hoisted_with_bins,
    })
}

fn write_project_sidecars(inputs: &LinkPhaseInputs<'_>) -> Result<(), LinkPhaseError> {
    let config = inputs.ctx.config;
    let phase_start = std::time::Instant::now();
    if crate::should_write_package_map(config, inputs.ctx.node_linker) {
        crate::package_map::write_package_map(
            inputs.sidecar_lockfile,
            &crate::package_map::PackageMapOptions {
                lockfile_dir: inputs.ctx.workspace_root,
                modules_dir: &config.modules_dir,
                package_map_type: config.node_package_map_type,
                layout: inputs.ctx.layout,
                project_manifests: inputs.project_manifests,
            },
        )
        .map_err(LinkPhaseError::WritePackageMap)?;
    } else if inputs.ctx.node_linker != NodeLinker::Hoisted {
        // A hoisted install writes its map from its own linker, which
        // runs after this one — see `should_write_hoisted_package_map`.
        // Only the linkers whose map this gate speaks for may take one
        // away.
        crate::package_map::remove_package_map(&config.modules_dir);
    }
    tracing::info!(
        target: "pacquet::install::phase",
        phase = "link.package_map",
        elapsed_ms = phase_start.elapsed().as_millis() as u64,
        "phase complete",
    );
    if matches!(inputs.ctx.node_linker, NodeLinker::Pnp) {
        crate::write_pnp_file(
            inputs.sidecar_lockfile,
            inputs.ctx.workspace_root,
            config,
            inputs.ctx.layout,
            inputs.project_manifests,
        )
        .map_err(LinkPhaseError::WritePnpFile)?;
    }
    Ok(())
}

fn link_hoisted_projects<Reporter: self::Reporter>(
    inputs: &mut LinkPhaseInputs<'_>,
    skipped: &mut SkippedSnapshots,
) -> Result<crate::HoistedLinkerOutput, LinkPhaseError> {
    let config = inputs.ctx.config;
    let hoisted = inputs
        .ctx
        .is_hoisted()
        .then(|| {
            run_hoisted_linker::<Reporter>(
                &HoistedLinkerInputs {
                    config,
                    lockfile: inputs.lockfile,
                    current_lockfile: inputs.current_lockfile,
                    current_hoisted_locations: inputs.prior_hoisted_locations,
                    build_present_packages: inputs.build_present_packages,
                    prior_unbuilt_builds: inputs.prior_unbuilt_builds,
                    layout: inputs.ctx.layout,
                    importers: &inputs.lockfile.importers,
                    dependency_groups: inputs.dependency_groups,
                    project_manifests: inputs.project_manifests,
                    package_map_project_manifests: inputs.package_map_project_manifests,
                    walker_lockfile_dir: inputs.ctx.workspace_root,
                    symlink_workspace_root: inputs.symlink_root,
                    host_node: inputs.host_node,
                    supported_architectures: inputs.supported_architectures,
                    cas_paths_by_pkg_id: inputs.cas_paths_by_pkg_id.take(),
                    logged_methods: inputs.ctx.logged_methods,
                    requester: inputs.ctx.requester,
                    requires_build_by_snapshot: inputs.requires_build_by_snapshot,
                    dir_clone_cache: inputs.ctx.dir_clone_cache,
                },
                skipped,
            )
            .map_err(LinkPhaseError::from)
        })
        .transpose()?
        .unwrap_or_default();

    Ok(hoisted)
}

/// Publicly hoisted *workspace* packages are the one source of root
/// bins nothing else shims: every importer's direct-dep bins were
/// written by `SymlinkDirectDependencies`, and publicly hoisted regular
/// packages go through the post-build top-level pass
/// (`publicly_hoisted_for_post_build`) — but that pass resolves bins out
/// of virtual-store slots, which a workspace project doesn't have.
/// Collected before [`write_hoist_links`] consumes the plan; shimmed after
/// the hoist symlinks land.
fn public_workspace_bin_deps(plan: Option<&HoistPlan>) -> Vec<(String, PathBuf)> {
    plan.map(|plan| {
        plan.result
            .hoisted_workspace_aliases
            .iter()
            .filter(|(_, kind, _)| matches!(kind, pnpm_modules_yaml::HoistKind::Public))
            .map(|(alias, _, project_dir)| (alias.clone(), project_dir.clone()))
            .collect()
    })
    .unwrap_or_default()
}

/// Symlink the hoist plan's aliases into the private
/// (`<virtual_store>/node_modules`) and public (`<root>/node_modules`)
/// targets, then shim the private side's bins.
///
/// Enabling the global virtual store does not move the private target:
/// pacquet leaves `virtual_store_dir` at its project-local (or
/// yaml-pinned) value and routes the shared root through
/// `global_virtual_store_dir` instead — see
/// [`Config::apply_global_virtual_store_derivation`]. Only the symlink
/// *target* under the slot dir is GVS-aware, which `layout` resolves.
fn write_hoist_links(
    plan: HoistPlan,
    config: &Config,
    layout: &VirtualStoreLayout,
    link_options: &LinkBinsOptions,
) -> Result<HoistLinks, LinkPhaseError> {
    let HoistPlan { graph, result, skipped, .. } = plan;
    let private_hoist_dir = config.virtual_store_dir.join("node_modules");
    let public_hoist_dir = config.modules_dir.clone();
    symlink_hoisted_dependencies(
        &result.hoisted_dependencies_by_node_id,
        &result.hoisted_workspace_aliases,
        &graph,
        layout,
        &private_hoist_dir,
        &public_hoist_dir,
        &skipped,
    )
    .map_err(LinkPhaseError::HoistSymlink)?;
    link_direct_dep_bins_resolved(
        &private_hoist_dir,
        &crate::resolve_hoisted_bin_deps(layout, &result.hoisted_aliases_with_bins),
        link_options,
    )
    .map_err(LinkPhaseError::HoistLinkBins)?;
    Ok(HoistLinks {
        hoisted_dependencies: result.hoisted_dependencies,
        publicly_hoisted_with_bins: result.publicly_hoisted_aliases_with_bins,
    })
}
