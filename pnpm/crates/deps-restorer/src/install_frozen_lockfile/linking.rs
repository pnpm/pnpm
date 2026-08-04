//! The frozen install's link phase: reconcile what the previous install
//! left behind, materialize the importer-visible tree — either the
//! hoisted `node_modules` hierarchy or the isolated symlink layout with
//! its hoist and bin passes — and write the module-resolution sidecars.
//!
//! Runs once the virtual store is populated and before the build phase,
//! which needs both the linked tree and the hoisted package roots this
//! reports.

use super::{
    HoistPlan, HoistedLinkerInputs, HoistedLinkerOutput, InstallFrozenLockfileError,
    collect_public_hoist_targets, compute_hoist_plan, run_hoisted_linker,
    workspace_packages_for_hoist,
};
use crate::{
    CasPathsByPkgId, LinkVirtualStoreBins, PackageManifests, SkippedSnapshots,
    SymlinkDirectDependencies, VirtualStoreLayout, link_direct_dep_bins_resolved,
    link_root_component_members, symlink_hoisted_dependencies,
};
use pacquet_config::{Config, NodeLinker};
use pacquet_lockfile::{Lockfile, PackageKey, PackageMetadata, ProjectSnapshot, SnapshotEntry};
use pacquet_modules_yaml::IncludedDependencies;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_reporter::{LogEvent, LogLevel, Reporter, StatsLog, StatsMessage};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::atomic::AtomicU8,
};

pub(super) struct LinkPhaseInputs<'a> {
    pub config: &'static Config,
    pub layout: &'a VirtualStoreLayout,
    pub lockfile: &'a Lockfile,
    pub current_lockfile: Option<&'a Lockfile>,
    pub snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    pub packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    pub importers: &'a HashMap<String, ProjectSnapshot>,
    pub project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub package_map_project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub dependency_groups: &'a [DependencyGroup],
    pub package_manifests: &'a PackageManifests,
    pub cas_paths_by_pkg_id: Option<CasPathsByPkgId>,
    pub extra_node_paths: &'a [String],
    pub workspace_root: &'a Path,
    pub requester: &'a str,
    pub node_linker: NodeLinker,
    pub is_hoisted: bool,
    pub prune_orphans: bool,
    pub prior_hoisted_dependencies: Option<&'a crate::HoistedDependencies>,
    pub host_node: Option<&'a (bool, String)>,
    pub supported_architectures: Option<&'a pacquet_package_is_installable::SupportedArchitectures>,
    pub logged_methods: &'a AtomicU8,
}

/// What the link phase hands to the build phase and the caller's
/// `.modules.yaml` writer.
pub(super) struct LinkPhaseOutput {
    pub hoisted_dependencies: crate::HoistedDependencies,
    pub hoisted_locations: BTreeMap<String, Vec<String>>,
    pub hoisted_pkg_roots_by_key: Option<HashMap<PackageKey, Vec<PathBuf>>>,
    pub publicly_hoisted_for_post_build: Vec<String>,
}

pub(super) fn run_link_phase<Reporter: self::Reporter>(
    inputs: LinkPhaseInputs<'_>,
    skipped: &mut SkippedSnapshots,
) -> Result<LinkPhaseOutput, InstallFrozenLockfileError> {
    let LinkPhaseInputs {
        config,
        layout,
        lockfile,
        current_lockfile,
        snapshots,
        packages,
        importers,
        project_manifests,
        package_map_project_manifests,
        dependency_groups,
        package_manifests,
        cas_paths_by_pkg_id,
        extra_node_paths,
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

    // Pre-compute the hoist plan so the dedupe pass inside
    // `SymlinkDirectDependencies` can fold publicly-hoisted aliases
    // into root's target map — pacquet runs hoist *after*
    // `SymlinkDirectDependencies`, so without this the dedupe map
    // only sees root's direct deps and a non-root importer's
    // direct dep that would land at root via public-hoist stays
    // un-deduped. The full `HoistResult` is also threaded to the
    // on-disk hoist pass below so the traversal isn't run twice.
    // `hoist-workspace-packages`: named non-root projects become
    // hoist candidates whose links point at the project dirs.
    let hoisted_workspace_packages = config
        .hoist_workspace_packages
        .then(|| workspace_packages_for_hoist(workspace_root, project_manifests));
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

    // Reconcile before linking: stale direct-dep links and
    // orphaned hoist links must vacate their slots so the relink +
    // rehoist below can claim them. The hoisted linker is excluded
    // — its previous-graph diff removes orphans and emits the
    // `pnpm:stats` `removed` event itself (see
    // [`crate::link_hoisted_modules()`]); on the isolated linker
    // the event fires here, so every install carries exactly one,
    // pairing the `added` emitted in `CreateVirtualStore`.
    //
    // `virtual_store_only` skips reconciliation for the same reason
    // it skips linking below: it never creates importer or hoist
    // links, so there is nothing of its own to reconcile.
    if !is_hoisted && !config.virtual_store_only {
        let removed_count = match current_lockfile {
            Some(current) => crate::PruneStaleModules {
                config,
                workspace_root,
                wanted_lockfile: lockfile,
                current_lockfile: current,
                prior_hoisted_dependencies,
                included_groups: dependency_groups,
                prune_orphans,
            }
            .run::<Reporter>()
            .map_err(InstallFrozenLockfileError::PruneStaleModules)?,
            None => 0,
        };
        Reporter::emit(&LogEvent::Stats(StatsLog {
            level: LogLevel::Debug,
            message: StatsMessage::Removed { prefix: requester.to_owned(), removed: removed_count },
        }));
    }

    // `virtual_store_only` stops here: the virtual store is
    // populated, but nothing downstream of it — importer symlinks,
    // per-slot bins, root components — gets linked.
    if !is_hoisted && !config.virtual_store_only {
        // Importer ids backed by the install's own declared
        // projects. These may legitimately live outside the
        // lockfile dir (Bit's capsule installs pass such
        // projects), so they bypass the malformed-lockfile
        // importer-key rejection.
        let trusted_importer_ids: std::collections::HashSet<String> = project_manifests
            .iter()
            .map(|(project_dir, _)| {
                pacquet_workspace::importer_id_from_root_dir(workspace_root, project_dir)
            })
            .collect();
        SymlinkDirectDependencies {
            config,
            layout,
            importers,
            packages,
            dependency_groups: dependency_groups.iter().copied(),
            workspace_root,
            skipped,
            link_only: false,
            public_hoist_targets: public_hoist_targets.as_ref(),
            trusted_importer_ids: Some(&trusted_importer_ids),
            extra_node_paths,
        }
        .run::<Reporter>()
        .map_err(InstallFrozenLockfileError::SymlinkDirectDependencies)?;

        // Bit "root components": make each root's injected members
        // mutually reachable. Gated on
        // `installConfig.hoistingLimits: "workspaces"`, so it is a
        // no-op for every non-Bit install. See
        // [`link_root_component_members`]. `project_manifests` keys
        // are project directories; map each back to its lockfile
        // importer id so the set lines up with `importers`.
        let root_component_importers: std::collections::HashSet<String> = project_manifests
            .iter()
            .filter(|(_, manifest)| {
                manifest.install_config_hoisting_limits() == Some(crate::HOISTING_LIMITS_WORKSPACES)
            })
            .map(|(project_dir, _)| {
                pacquet_workspace::importer_id_from_root_dir(workspace_root, project_dir)
            })
            .collect();
        link_root_component_members(
            layout,
            importers,
            &root_component_importers,
            dependency_groups,
            skipped,
        )
        .map_err(InstallFrozenLockfileError::LinkRootComponentMembers)?;

        // Link the bins of each virtual-store slot's children into the
        // slot's own `node_modules/.bin`.
        // Done before `importing_done` so reporters see the import phase
        // close only after every link (including per-slot bins) is in
        // place. The manifest map threaded from `CreateVirtualStore`
        // lets the linker hit `pkgFilesIndex.manifest` directly instead
        // of re-reading every child's `package.json` from disk.
        //
        // Both passes are gated by `!is_hoisted`: under
        // `nodeLinker: hoisted` there is no virtual store
        // (`CreateVirtualStore` skipped slot writes), and the
        // bin links go into `<parent>/node_modules/.bin` for
        // every hoist location instead. The hoisted linker
        // ([`crate::link_hoisted_modules()`], called below) does
        // its own per-`node_modules` bin pass while walking the
        // hierarchy, routing both link phases through the hoisted
        // linker.
        LinkVirtualStoreBins {
            layout,
            snapshots,
            packages,
            package_manifests,
            skipped,
            extra_node_paths,
        }
        .run()
        .map_err(InstallFrozenLockfileError::LinkVirtualStoreBins)?;
    }

    // Hoisted-linker materialization. Replaces the isolated
    // [`crate::SymlinkDirectDependencies`] +
    // [`crate::LinkVirtualStoreBins`] pair when
    // `nodeLinker: hoisted` is in effect: the dep-graph walker
    // computes per-package directories (with conflict-aware
    // nesting), and the linker imports CAS files into those
    // directories from
    // [`CreateVirtualStoreOutput::cas_paths_by_pkg_id`] which
    // was populated above with `node_linker = Hoisted`.
    //
    // `hoisted_locations` is the per-depPath list of
    // lockfile-relative directories the walker emits. Threaded
    // through [`InstallFrozenLockfileOutput`] so
    // [`crate::Install::run`] can persist it into
    // `.modules.yaml.hoisted_locations` (rebuild reads it back
    // and surfaces `MISSING_HOISTED_LOCATIONS` if it's gone).
    //
    // `pkg_roots_by_key` is a per-snapshot override for
    // `BuildModules`'s `pkgRoot` lookup. Populated from the
    // walker's [`crate::DependenciesGraphNode::dir`] values so
    // the build phase can `cd` into the on-disk hoisted
    // directory instead of computing a virtual-store slot path
    // that doesn't exist under hoisted. `None` (and an empty
    // `hoisted_locations`) for the isolated linker. See
    // [`crate::BuildModules::pkg_roots_by_key`] for why a snapshot
    // can map to more than one directory and which writes have to
    // reach all of them.
    let HoistedLinkerOutput { hoisted_locations, hoisted_pkg_roots_by_key } =
        if is_hoisted && !config.virtual_store_only {
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
                    symlink_workspace_root: workspace_root,
                    host_node,
                    supported_architectures,
                    cas_paths_by_pkg_id,
                    logged_methods,
                    requester,
                },
                skipped,
            )
            .map_err(InstallFrozenLockfileError::from)?
        } else {
            HoistedLinkerOutput::default()
        };

    // Hoist transitive deps into `<virtual_store>/node_modules`
    // (private hoist) and/or `<root>/node_modules` (public hoist).
    //
    // The guard is `hoistPattern != null || publicHoistPattern != null`
    // — `Some(empty)` is a valid disabled state for one side but
    // not the other, so the guard checks `is_some()` on the field
    // (not `Vec` length). With pacquet's defaults both sides are
    // `Some(non-empty)`, so the pass runs by default.
    // Stashed across the hoist pass for the post-`BuildModules`
    // top-level bin link. Isolated-linker public-hoist promotes
    // a transitive dep alias to `<root>/node_modules/<alias>`
    // where it competes for the same `<root>/node_modules/.bin`
    // slot as the root importer's direct deps. Per
    // pnpm/pacquet#342 the direct dep's bin must win. The post-build pass below
    // takes both direct + hoisted candidate lists so
    // `pacquet_cmd_shim::pick_winner` (private)'s [`BinOrigin`] tier
    // resolves the conflict in one call. Empty means there's
    // no public-hoist (no patterns set, hoisted linker, or
    // `Some(empty)`-vs-`None` short-circuit).
    let mut publicly_hoisted_for_post_build: Vec<String> = Vec::new();
    // Isolated-linker hoist pass: shamefully-hoist + private
    // hoist into the virtual store. Skipped under hoisted —
    // the hoisted linker materialized the project tree above
    // and there's no virtual store to point hoist symlinks at,
    // so no new isolated-hoist results are produced when no
    // `hoistPattern` / `publicHoistPattern` is configured.
    //
    // The traversal itself ran upthread (`pre_hoist`) so the dedupe
    // pass in `SymlinkDirectDependencies` could see public-hoist
    // targets; here we consume the same plan to write the
    // symlinks on disk and emit the per-side bin shims.
    let hoisted_dependencies = if let Some(plan) = pre_hoist {
        let HoistPlan { graph, result, skipped: hoist_skipped, .. } = plan;
        // Public-hoist target is the project's root
        // `node_modules` (= `config.modules_dir`).
        // Private-hoist target is the project-local
        // `<root>/node_modules/.pnpm/node_modules` —
        // pacquet's `config.virtual_store_dir` always
        // resolves there even with GVS enabled: pacquet keeps
        // `virtual_store_dir` project-local and
        // routes the GVS-shared root through
        // `global_virtual_store_dir` instead — see
        // [`Config::apply_global_virtual_store_derivation`].
        // The symlink *target* (under the slot dir)
        // does need to be GVS-aware, which the
        // `VirtualStoreLayout` handle below provides.
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
        .map_err(InstallFrozenLockfileError::HoistSymlink)?;
        // Private-side bins → `<vs>/node_modules/.bin`.
        // Reuses the rayon-parallel `link_direct_dep_bins`
        // shape (read each location's `package.json`, fan out
        // to `link_bins_of_packages`).
        link_direct_dep_bins_resolved(
            &private_dir,
            &crate::resolve_hoisted_bin_deps(layout, &result.hoisted_aliases_with_bins),
            extra_node_paths,
        )
        .map_err(InstallFrozenLockfileError::HoistLinkBins)?;
        // Stash the public-hoist alias list for the
        // post-`BuildModules` top-level bin link, which re-links
        // with the [`BinOrigin`] tier so a direct dep's bin wins
        // outright over a publicly-hoisted bin with a lexically
        // smaller name. The re-link runs after `buildModules`.
        publicly_hoisted_for_post_build = result.publicly_hoisted_aliases_with_bins;
        result.hoisted_dependencies
    } else {
        crate::HoistedDependencies::new()
    };

    let included = IncludedDependencies {
        dependencies: dependency_groups.contains(&DependencyGroup::Prod),
        dev_dependencies: dependency_groups.contains(&DependencyGroup::Dev),
        optional_dependencies: dependency_groups.contains(&DependencyGroup::Optional),
    };
    if crate::should_write_package_map(config, node_linker) {
        let filtered_lockfile = crate::filter_lockfile_for_current(lockfile, included, skipped);
        crate::package_map::write_package_map(
            &filtered_lockfile,
            &crate::package_map::PackageMapOptions {
                lockfile_dir: workspace_root,
                modules_dir: &config.modules_dir,
                package_map_type: config.node_package_map_type,
                layout,
                project_manifests,
            },
        )
        .map_err(InstallFrozenLockfileError::WritePackageMap)?;
    }
    // See `install_with_fresh_lockfile::linking` for why
    // `virtual_store_only` suppresses the loader.
    if matches!(node_linker, NodeLinker::Pnp) && !config.virtual_store_only {
        let filtered_lockfile = crate::filter_lockfile_for_current(lockfile, included, skipped);
        crate::write_pnp_file(
            &filtered_lockfile,
            workspace_root,
            config,
            layout,
            project_manifests,
        )
        .map_err(InstallFrozenLockfileError::WritePnpFile)?;
    }
    Ok(LinkPhaseOutput {
        hoisted_dependencies,
        hoisted_locations,
        hoisted_pkg_roots_by_key,
        publicly_hoisted_for_post_build,
    })
}
