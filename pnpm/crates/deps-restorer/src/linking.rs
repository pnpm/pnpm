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
        HoistPlan, HoistedLinkerError, HoistedLinkerInputs, HoistedLinkerOutput,
        collect_public_hoist_targets, compute_hoist_plan, run_hoisted_linker,
        workspace_packages_for_hoist,
    },
    link_direct_dep_bins_resolved, link_root_component_members, symlink_hoisted_dependencies,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_cmd_shim::LinkBinsOptions;
use pnpm_config::{Config, NodeLinker};
use pnpm_lockfile::{Lockfile, PackageKey, PackageMetadata, ProjectSnapshot, SnapshotEntry};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{LogEvent, LogLevel, Reporter, StatsLog, StatsMessage};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::atomic::AtomicU8,
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
    pub config: &'static Config,
    pub layout: &'a VirtualStoreLayout,
    pub lockfile: &'a Lockfile,
    pub current_lockfile: Option<&'a Lockfile>,
    pub snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    /// Restricts per-slot bin linking to this install's materialized
    /// snapshots. `None` keeps rebuild's all-slot behavior.
    pub materialized_snapshots: Option<&'a [PackageKey]>,
    pub packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    pub importers: &'a HashMap<String, ProjectSnapshot>,
    pub project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub package_map_project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub dependency_groups: &'a [DependencyGroup],
    pub package_manifests: &'a PackageManifests,
    pub cas_paths_by_pkg_id: Option<CasPathsByPkgId>,
    pub link_options: &'a LinkBinsOptions,
    /// Anchor for each importer's `node_modules`. The frozen path uses
    /// `workspace_root`; the fresh path uses `modules_dir.parent()`,
    /// because its tests relocate `modules_dir` away from the manifest.
    pub symlink_root: &'a Path,
    /// Lockfile dir, for the sidecars and the hoisted walker.
    pub workspace_root: &'a Path,
    /// Importer ids allowed to live outside the lockfile dir (Bit's
    /// capsule installs).
    pub trusted_importer_ids: &'a std::collections::HashSet<String>,
    /// Importers declaring `installConfig.hoistingLimits: "workspaces"`.
    pub root_component_importers: &'a std::collections::HashSet<String>,
    /// The lockfile the module-resolution sidecars describe. The frozen
    /// path filters to the current install first; the fresh path already
    /// holds a materialization closure.
    pub sidecar_lockfile: &'a Lockfile,
    pub requester: &'a str,
    pub node_linker: NodeLinker,
    pub is_hoisted: bool,
    pub prune_orphans: bool,
    pub prior_hoisted_dependencies: Option<&'a crate::HoistedDependencies>,
    pub host_node: Option<&'a crate::materialization_plan::HostNode>,
    pub supported_architectures: Option<&'a pnpm_package_is_installable::SupportedArchitectures>,
    pub logged_methods: &'a AtomicU8,
}

/// What the link phase hands to the build phase and the caller's
/// `.modules.yaml` writer.
pub struct LinkPhaseOutput {
    pub hoisted_dependencies: crate::HoistedDependencies,
    pub hoisted_locations: BTreeMap<String, Vec<String>>,
    pub hoisted_pkg_roots_by_key: Option<HashMap<PackageKey, Vec<PathBuf>>>,
    pub publicly_hoisted_for_post_build: Vec<String>,
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
    let LinkPhaseInputs {
        symlink_root,
        trusted_importer_ids,
        root_component_importers,
        sidecar_lockfile,
        config,
        layout,
        lockfile,
        current_lockfile,
        snapshots,
        materialized_snapshots,
        packages,
        importers,
        project_manifests,
        package_map_project_manifests,
        dependency_groups,
        package_manifests,
        cas_paths_by_pkg_id,
        link_options,
        workspace_root,
        requester,
        node_linker,
        is_hoisted,
        prune_orphans,
        prior_hoisted_dependencies,
        host_node,
        supported_architectures,
        logged_methods,
    } = inputs;

    // `hoistWorkspacePackages`: named non-root projects become hoist
    // candidates whose links point at the project dirs.
    let hoisted_workspace_packages = config
        .hoist_workspace_packages
        .then(|| workspace_packages_for_hoist(workspace_root, project_manifests));
    // Planned here, ahead of `SymlinkDirectDependencies`, so that pass's
    // dedupe map can see publicly-hoisted aliases: the on-disk hoist runs
    // *after* it, and without the plan the map holds only root's direct
    // deps — leaving a non-root importer's dep that public-hoist would
    // land at root un-deduped. The plan is threaded to the hoist pass
    // below so the traversal runs once.
    let pre_hoist = compute_hoist_plan(
        config,
        snapshots,
        packages,
        importers,
        dependency_groups,
        skipped,
        is_hoisted,
        hoisted_workspace_packages.as_ref(),
    );
    let public_hoist_targets: Option<BTreeMap<String, PathBuf>> = pre_hoist
        .as_ref()
        .map(|plan| collect_public_hoist_targets(&plan.result, &plan.graph, layout, &plan.skipped));

    // Reconcile before linking: stale direct-dep links and orphaned
    // hoist links must vacate their slots so the relink + rehoist below
    // can claim them. Excluded under the hoisted linker, whose
    // previous-graph diff removes orphans and emits the `pnpm:stats`
    // `removed` event itself (see [`crate::link_hoisted_modules()`]) —
    // so every install carries exactly one, pairing the `added` emitted
    // in `CreateVirtualStore`. Excluded under `virtual_store_only` too:
    // it creates no importer or hoist links, so it has neither anything
    // to reconcile nor anything to link.
    if !is_hoisted && !config.virtual_store_only {
        let removed_count = match current_lockfile {
            Some(current) => crate::PruneStaleModules {
                config,
                workspace_root: symlink_root,
                wanted_lockfile: lockfile,
                current_lockfile: current,
                prior_hoisted_dependencies,
                included_groups: dependency_groups,
                prune_orphans,
            }
            .run::<Reporter>()
            .map_err(LinkPhaseError::PruneStaleModules)?,
            None => 0,
        };
        Reporter::emit(&LogEvent::Stats(StatsLog {
            level: LogLevel::Debug,
            message: StatsMessage::Removed { prefix: requester.to_owned(), removed: removed_count },
        }));

        SymlinkDirectDependencies {
            config,
            layout,
            importers,
            packages,
            dependency_groups: dependency_groups.iter().copied(),
            workspace_root: symlink_root,
            skipped,
            link_only: false,
            public_hoist_targets: public_hoist_targets.as_ref(),
            trusted_importer_ids: Some(trusted_importer_ids),
            link_options,
        }
        .run::<Reporter>()
        .map_err(LinkPhaseError::SymlinkDirectDependencies)?;

        // Bit "root components" — a no-op unless an importer declared
        // `installConfig.hoistingLimits: "workspaces"`.
        link_root_component_members(
            layout,
            importers,
            snapshots,
            root_component_importers,
            dependency_groups,
            skipped,
        )
        .map_err(LinkPhaseError::LinkRootComponentMembers)?;
    }

    if !is_hoisted {
        // Link the bins of each virtual-store slot's children into the
        // slot's own `node_modules/.bin`, before `importing_done` so
        // reporters see the import phase close only once every link is
        // in place. Skipped under `nodeLinker: hoisted`, which has no
        // virtual store to link into and whose linker below runs its own
        // per-`node_modules` bin pass while walking the hierarchy.
        //
        // Unlike every other pass here this one also runs under
        // `virtual_store_only`: the links it writes live inside the
        // virtual store, and the build phase — which `pnpm fetch` still
        // runs — resolves a dependency's sibling bin through them.
        LinkVirtualStoreBins {
            layout,
            snapshots,
            selected_snapshots: materialized_snapshots,
            packages,
            package_manifests,
            skipped,
            link_options,
        }
        .run()
        .map_err(LinkPhaseError::LinkVirtualStoreBins)?;
    }

    // Nothing below this point runs under `virtual_store_only`: it
    // materializes the importer-visible tree and the sidecars that
    // describe it, and the hoist pass writes into `config.modules_dir`
    // — a project this mode is meant to leave alone.
    if config.virtual_store_only {
        return Ok(LinkPhaseOutput {
            hoisted_dependencies: crate::HoistedDependencies::new(),
            hoisted_locations: BTreeMap::new(),
            hoisted_pkg_roots_by_key: None,
            publicly_hoisted_for_post_build: Vec::new(),
        });
    }

    // Hoisted-linker materialization: replaces the
    // [`crate::SymlinkDirectDependencies`] + [`crate::LinkVirtualStoreBins`]
    // pair above when `nodeLinker: hoisted` is in effect. The dep-graph
    // walker computes per-package directories (with conflict-aware
    // nesting) and the linker imports CAS files into them from
    // [`CreateVirtualStoreOutput::cas_paths_by_pkg_id`], populated above
    // with `node_linker = Hoisted`. Both outputs stay empty for the
    // isolated linker — see [`HoistedLinkerOutput`] for what they carry.
    let HoistedLinkerOutput { hoisted_locations, hoisted_pkg_roots_by_key } = if is_hoisted {
        run_hoisted_linker::<Reporter>(
            HoistedLinkerInputs {
                config,
                lockfile,
                current_lockfile,
                layout,
                importers,
                dependency_groups,
                project_manifests,
                package_map_project_manifests,
                walker_lockfile_dir: workspace_root,
                symlink_workspace_root: symlink_root,
                host_node,
                supported_architectures,
                cas_paths_by_pkg_id,
                logged_methods,
                requester,
            },
            skipped,
        )
        .map_err(LinkPhaseError::from)?
    } else {
        HoistedLinkerOutput::default()
    };

    // Public-hoist promotes a transitive dep's alias to
    // `<root>/node_modules/<alias>`, where its bin competes for the same
    // `<root>/node_modules/.bin` slot as a root direct dep's. Per
    // pnpm/pacquet#342 the direct dep must win, so the aliases are stashed
    // here for the post-`BuildModules` top-level bin link, which takes
    // both candidate lists and lets `pick_winner`'s [`BinOrigin`] tier
    // settle it in one call. Empty when nothing was publicly hoisted.
    let mut publicly_hoisted_for_post_build: Vec<String> = Vec::new();
    // Write the plan to disk: shamefully-hoist + private hoist into the
    // virtual store, plus the per-side bin shims. `pre_hoist` is `None`
    // under the hoisted linker, which materialized the tree itself and
    // has no virtual store to point hoist symlinks at.
    let hoisted_dependencies = if let Some(plan) = pre_hoist {
        let HoistPlan { graph, result, skipped: hoist_skipped, .. } = plan;
        // `virtual_store_dir` stays project-local even with GVS enabled
        // — the shared root is routed through `global_virtual_store_dir`
        // instead (see [`Config::apply_global_virtual_store_derivation`])
        // — so the private-hoist target is always
        // `<root>/node_modules/.pnpm/node_modules`. Only the symlink
        // *target* under the slot dir is GVS-aware, which `layout`
        // resolves.
        let private_dir = config.virtual_store_dir.join("node_modules");
        let public_dir = config.modules_dir.clone();
        symlink_hoisted_dependencies(
            &result.hoisted_dependencies_by_node_id,
            &result.hoisted_workspace_aliases,
            &graph,
            layout,
            &private_dir,
            &public_dir,
            &hoist_skipped,
        )
        .map_err(LinkPhaseError::HoistSymlink)?;
        // Private-side bins → `<vs>/node_modules/.bin`.
        link_direct_dep_bins_resolved(
            &private_dir,
            &crate::resolve_hoisted_bin_deps(layout, &result.hoisted_aliases_with_bins),
            link_options,
        )
        .map_err(LinkPhaseError::HoistLinkBins)?;
        publicly_hoisted_for_post_build = result.publicly_hoisted_aliases_with_bins;
        result.hoisted_dependencies
    } else {
        crate::HoistedDependencies::new()
    };

    if crate::should_write_package_map(config, node_linker) {
        crate::package_map::write_package_map(
            sidecar_lockfile,
            &crate::package_map::PackageMapOptions {
                lockfile_dir: workspace_root,
                modules_dir: &config.modules_dir,
                package_map_type: config.node_package_map_type,
                layout,
                project_manifests,
            },
        )
        .map_err(LinkPhaseError::WritePackageMap)?;
    }
    if matches!(node_linker, NodeLinker::Pnp) {
        crate::write_pnp_file(sidecar_lockfile, workspace_root, config, layout, project_manifests)
            .map_err(LinkPhaseError::WritePnpFile)?;
    }
    // `SymlinkDirectDependencies` already linked direct-dep bins, but
    // hoisting runs after it. Re-walking each importer's `node_modules`
    // is what shims a publicly-hoisted *workspace package*'s bin: those
    // are recorded in the hoist result's `hoisted_workspace_aliases`,
    // which the post-build top-level link never sees.
    {
        let modules_basename = config.modules_dir.file_name().map_or_else(
            || std::ffi::OsString::from("node_modules"),
            std::ffi::OsStr::to_os_string,
        );
        for importer_id in trusted_importer_ids {
            let modules_dir =
                crate::symlink_direct_dependencies::importer_root_dir(symlink_root, importer_id)
                    .join(&modules_basename);
            let bins_dir = modules_dir.join(".bin");
            pnpm_cmd_shim::link_bins::<pnpm_cmd_shim::Host>(&modules_dir, &bins_dir, link_options)
                .map_err(LinkPhaseError::LinkBins)?;
        }
    }

    Ok(LinkPhaseOutput {
        hoisted_dependencies,
        hoisted_locations,
        hoisted_pkg_roots_by_key,
        publicly_hoisted_for_post_build,
    })
}
