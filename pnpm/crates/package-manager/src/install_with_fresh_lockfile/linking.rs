//! The fresh install's link phase: reconcile what the previous install
//! left behind, then materialize the importer-visible tree — either the
//! hoisted `node_modules` hierarchy or the isolated symlink layout with
//! its hoist and bin passes.
//!
//! Split out of [`super::InstallWithFreshLockfile::run`] so the
//! orchestrator reads as a sequence of install phases. Everything here
//! runs between `CreateVirtualStore` and the build phase.

use super::InstallWithFreshLockfileError;
use crate::{
    HoistedDependencies, LinkVirtualStoreBins, SkippedSnapshots, SymlinkDirectDependencies,
    VirtualStoreLayout, link_root_component_members,
};
use pacquet_cmd_shim::{Host, link_bins};
use pacquet_config::Config;
use pacquet_lockfile::Lockfile;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_reporter::{LogEvent, LogLevel, StatsLog, StatsMessage};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};

pub(super) struct LinkPhaseInputs<'a> {
    pub config: &'static Config,
    pub layout: &'a VirtualStoreLayout,
    /// The lockfile narrowed to what this install materializes.
    pub materialization_lockfile: &'a Lockfile,
    pub current_lockfile: Option<&'a Lockfile>,
    pub prior_hoisted_dependencies: Option<&'a HoistedDependencies>,
    pub importer_manifests: &'a BTreeMap<String, &'a PackageManifest>,
    pub dependency_groups: &'a [DependencyGroup],
    /// Importers whose own `node_modules` this install anchors links in.
    pub project_anchor_importer_ids: &'a HashSet<String>,
    /// Importers inside the materialization closure — the hoist plan's
    /// workspace-package candidates are drawn from these.
    pub materialization_importer_ids: &'a HashSet<String>,
    pub lockfile_dir: &'a Path,
    /// Anchor for every importer's `node_modules`. Derived from
    /// `config.modules_dir.parent()` rather than `lockfile_dir` — see
    /// [`run_link_phase`].
    pub symlink_root: &'a Path,
    pub package_manifests: &'a crate::PackageManifests,
    pub cas_paths_by_pkg_id: Option<crate::CasPathsByPkgId>,
    pub host_node: Option<&'a (bool, String)>,
    pub supported_architectures: Option<&'a pacquet_package_is_installable::SupportedArchitectures>,
    pub extra_node_paths: &'a [String],
    pub logged_methods: &'a std::sync::atomic::AtomicU8,
    pub requester: &'a str,
    pub is_hoisted: bool,
    /// See [`crate::PruneStaleModules::prune_orphans`].
    pub prune_orphans: bool,
}

/// What the link phase hands to the build phase and the caller's
/// `.modules.yaml` writer.
pub(super) struct LinkPhaseOutput {
    /// The publicly/privately-hoisted alias map under the isolated
    /// linker; empty under `nodeLinker: hoisted`, which writes the
    /// on-disk tree directly and reports through
    /// [`Self::hoisted_locations`] instead.
    pub hoisted_dependencies: HoistedDependencies,
    pub hoisted_locations: BTreeMap<String, Vec<String>>,
    /// Lets `BuildModules` `cd` into the hoisted on-disk dir. `None`
    /// under the isolated linker.
    pub hoisted_pkg_roots_by_key: Option<HashMap<pacquet_lockfile::PackageKey, Vec<PathBuf>>>,
    /// The public-hoist alias list the post-build top-level bin link
    /// consults so a direct dep's bin wins over a hoisted one.
    pub publicly_hoisted_for_post_build: Vec<String>,
}

/// Reconcile the previous install's links, then materialize the tree.
///
/// **The link passes anchor on `config.modules_dir.parent()`, not
/// `lockfile_dir`.** `SymlinkDirectDependencies` resolves each importer's
/// modules dir as `<workspace_root>/<importer_id>/<modules_basename>`,
/// which for the root importer (`.`) collapses to
/// `<workspace_root>/<modules_basename>`. The fresh-lockfile path's tests
/// parameterise `config.modules_dir` at a path that doesn't always live
/// under the manifest's directory, so anchoring on the
/// lockfile-dir-derived `workspace_root` would land symlinks at the wrong
/// path on those configurations. Where `config.modules_dir ==
/// <lockfile_dir>/node_modules` the two coincide; for an
/// explicitly-relocated `modules_dir` the symlinks land where the rest of
/// pacquet's install code (`.modules.yaml`, `LinkVirtualStoreBins`, ...)
/// already writes.
///
/// `virtual_store_only` stops after the virtual store is populated:
/// neither linker arm runs, so nothing is hoisted and no importer
/// symlinks or bins are created — and nothing of its own needs
/// reconciling either.
pub(super) fn run_link_phase<Reporter: pacquet_reporter::Reporter>(
    mut inputs: LinkPhaseInputs<'_>,
    skipped: &mut SkippedSnapshots,
) -> Result<LinkPhaseOutput, InstallWithFreshLockfileError> {
    let &LinkPhaseInputs {
        config,
        materialization_lockfile,
        current_lockfile,
        prior_hoisted_dependencies,
        dependency_groups,
        symlink_root,
        requester,
        is_hoisted,
        prune_orphans,
        ..
    } = &inputs;

    // Reconcile before linking: stale direct-dep links and orphaned
    // hoist links must vacate their slots so the relink + rehoist below
    // can claim them. The hoisted linker is excluded — its previous-graph
    // diff removes orphans and emits the `pnpm:stats` `removed` event
    // itself (see [`crate::link_hoisted_modules()`]); on the isolated
    // linker the event fires here, so every install carries exactly one,
    // pairing the `added` emitted in `CreateVirtualStore`.
    if !is_hoisted && !config.virtual_store_only {
        let removed_count = match current_lockfile {
            Some(current) => crate::PruneStaleModules {
                config,
                workspace_root: symlink_root,
                wanted_lockfile: materialization_lockfile,
                current_lockfile: current,
                prior_hoisted_dependencies,
                included_groups: dependency_groups,
                prune_orphans,
            }
            .run::<Reporter>()
            .map_err(InstallWithFreshLockfileError::PruneStaleModules)?,
            None => 0,
        };
        Reporter::emit(&LogEvent::Stats(StatsLog {
            level: LogLevel::Debug,
            message: StatsMessage::Removed { prefix: requester.to_owned(), removed: removed_count },
        }));
    }

    if config.virtual_store_only {
        return Ok(LinkPhaseOutput {
            hoisted_dependencies: HoistedDependencies::new(),
            hoisted_locations: BTreeMap::new(),
            hoisted_pkg_roots_by_key: None,
            publicly_hoisted_for_post_build: Vec::new(),
        });
    }

    if is_hoisted {
        let cas_paths_by_pkg_id = inputs.cas_paths_by_pkg_id.take();
        return link_hoisted::<Reporter>(&inputs, cas_paths_by_pkg_id, skipped);
    }
    link_isolated::<Reporter>(&inputs, skipped)
}

/// Under `nodeLinker: hoisted` the regular deps live as real directories
/// materialized by the hoisted linker, not as symlinks into the virtual
/// store. Route through the same walker + linker + `link_only` symlink
/// pass the frozen path uses, then skip the isolated-linker
/// public/private hoist and `LinkVirtualStoreBins` passes entirely — the
/// hoisted linker writes per-`node_modules` bins while walking the
/// hierarchy.
fn link_hoisted<Reporter: pacquet_reporter::Reporter>(
    inputs: &LinkPhaseInputs<'_>,
    cas_paths_by_pkg_id: Option<crate::CasPathsByPkgId>,
    skipped: &mut SkippedSnapshots,
) -> Result<LinkPhaseOutput, InstallWithFreshLockfileError> {
    let project_manifests = inputs
        .importer_manifests
        .iter()
        .filter(|(id, _)| inputs.project_anchor_importer_ids.contains(id.as_str()))
        .map(|(id, manifest)| (inputs.lockfile_dir.join(id), *manifest))
        .collect::<Vec<_>>();
    let package_map_project_manifests = inputs
        .importer_manifests
        .iter()
        .map(|(id, manifest)| (inputs.lockfile_dir.join(id), *manifest))
        .collect::<Vec<_>>();
    // Reuse the host probed once for the engine-name key, so a hoisted
    // install with installability constraints spawns `node --version`
    // only once. `None` when nothing constrained it (the engine check
    // then resolves against the host inside `run_hoisted_linker`,
    // matching the frozen path).
    let output = crate::install_frozen_lockfile::run_hoisted_linker::<Reporter>(
        crate::install_frozen_lockfile::HoistedLinkerInputs {
            config: inputs.config,
            lockfile: inputs.materialization_lockfile,
            current_lockfile: inputs.current_lockfile,
            layout: inputs.layout,
            importers: &inputs.materialization_lockfile.importers,
            dependency_groups: inputs.dependency_groups,
            project_manifests: &project_manifests,
            package_map_project_manifests: &package_map_project_manifests,
            walker_lockfile_dir: inputs.lockfile_dir,
            symlink_workspace_root: inputs.symlink_root,
            host_node: inputs.host_node,
            supported_architectures: inputs.supported_architectures,
            cas_paths_by_pkg_id,
            logged_methods: inputs.logged_methods,
            requester: inputs.requester,
        },
        skipped,
    )
    .map_err(InstallWithFreshLockfileError::from)?;
    Ok(LinkPhaseOutput {
        // The hoisted linker has no isolated-mode alias→kind adapter
        // shape, so it reports its placements through
        // `hoisted_locations` instead.
        hoisted_dependencies: HoistedDependencies::new(),
        hoisted_locations: output.hoisted_locations,
        hoisted_pkg_roots_by_key: output.hoisted_pkg_roots_by_key,
        publicly_hoisted_for_post_build: Vec::new(),
    })
}

/// The isolated linker: importer symlinks into the virtual store, the
/// public/private hoist, and the two bin passes.
fn link_isolated<Reporter: pacquet_reporter::Reporter>(
    inputs: &LinkPhaseInputs<'_>,
    skipped: &SkippedSnapshots,
) -> Result<LinkPhaseOutput, InstallWithFreshLockfileError> {
    let &LinkPhaseInputs {
        config,
        layout,
        materialization_lockfile,
        importer_manifests,
        dependency_groups,
        project_anchor_importer_ids,
        materialization_importer_ids,
        lockfile_dir,
        symlink_root,
        package_manifests,
        extra_node_paths,
        ..
    } = inputs;

    // Pre-compute the hoist plan so the dedupe pass in
    // `SymlinkDirectDependencies` can fold publicly-hoisted aliases into
    // root's target map — same shape as the frozen-lockfile path. The
    // `HoistResult` is reused for the on-disk hoist phase below, so the
    // traversal runs once. Under `hoist-workspace-packages`, named
    // non-root projects become hoist candidates whose links point at the
    // project dirs (`importer_manifests` is keyed by importer id).
    let hoisted_workspace_packages = config.hoist_workspace_packages.then(|| {
        importer_manifests
            .iter()
            .filter(|(id, _)| materialization_importer_ids.contains(id.as_str()))
            .filter(|(id, _)| id.as_str() != ".")
            .filter_map(|(id, manifest)| {
                let name = manifest.value().get("name")?.as_str()?;
                Some((
                    name.to_string(),
                    crate::symlink_direct_dependencies::importer_root_dir(lockfile_dir, id),
                ))
            })
            .collect::<indexmap::IndexMap<_, _>>()
    });
    let pre_hoist = crate::install_frozen_lockfile::compute_hoist_plan(
        config,
        materialization_lockfile.snapshots.as_ref(),
        materialization_lockfile.packages.as_ref(),
        &materialization_lockfile.importers,
        dependency_groups,
        skipped,
        false,
        hoisted_workspace_packages.as_ref(),
    );
    let public_hoist_targets: Option<BTreeMap<String, PathBuf>> = pre_hoist.as_ref().map(|plan| {
        crate::install_frozen_lockfile::collect_public_hoist_targets(
            &plan.result,
            &plan.graph,
            layout,
            &plan.skipped,
        )
    });

    // Importer ids backed by the install's own declared projects
    // (`importer_manifests` is keyed by importer id). These may
    // legitimately live outside the lockfile dir (Bit's capsule installs
    // pass such projects), so they bypass the malformed-lockfile
    // importer-key rejection.
    let trusted_importer_ids: HashSet<String> = project_anchor_importer_ids.clone();
    SymlinkDirectDependencies {
        config,
        layout,
        importers: &materialization_lockfile.importers,
        packages: materialization_lockfile.packages.as_ref(),
        dependency_groups: dependency_groups.iter().copied(),
        workspace_root: symlink_root,
        skipped,
        link_only: false,
        public_hoist_targets: public_hoist_targets.as_ref(),
        trusted_importer_ids: Some(&trusted_importer_ids),
        extra_node_paths,
    }
    .run::<Reporter>()
    .map_err(InstallWithFreshLockfileError::SymlinkDirectDependencies)?;

    // Bit "root components": make each root's injected members mutually
    // reachable. Gated on `installConfig.hoistingLimits: "workspaces"`,
    // so it is a no-op for every non-Bit install. See
    // [`link_root_component_members`].
    let root_component_importers: HashSet<String> = importer_manifests
        .iter()
        .filter(|(id, _)| project_anchor_importer_ids.contains(id.as_str()))
        .filter(|(_, manifest)| {
            manifest.install_config_hoisting_limits() == Some(crate::HOISTING_LIMITS_WORKSPACES)
        })
        .map(|(id, _)| id.clone())
        .collect();
    link_root_component_members(
        layout,
        &materialization_lockfile.importers,
        &root_component_importers,
        dependency_groups,
        skipped,
    )
    .map_err(InstallWithFreshLockfileError::LinkRootComponentMembers)?;

    // On-disk hoist phase. Mirrors the frozen-install block in
    // `install_frozen_lockfile.rs`: symlink the publicly + privately
    // hoisted aliases into their target dirs, then link private-side bins
    // into `<vs>/node_modules/.bin`. Public-side bin precedence is
    // handled implicitly by the per-importer `link_bins` pass below,
    // which walks both direct-dep and public-hoist symlinks in root's
    // `node_modules/`.
    let mut publicly_hoisted_for_post_build = Vec::new();
    let hoisted_dependencies = if let Some(plan) = pre_hoist {
        let crate::install_frozen_lockfile::HoistPlan {
            graph, result, skipped: hoist_skipped, ..
        } = plan;
        let private_dir = config.virtual_store_dir.join("node_modules");
        let public_dir = config.modules_dir.clone();
        crate::symlink_hoisted_dependencies(
            &result.hoisted_dependencies_by_node_id,
            &result.hoisted_workspace_aliases,
            &graph,
            layout,
            &private_dir,
            &public_dir,
            &hoist_skipped,
        )
        .map_err(InstallWithFreshLockfileError::HoistSymlink)?;
        crate::link_direct_dep_bins_resolved(
            &private_dir,
            &crate::resolve_hoisted_bin_deps(layout, &result.hoisted_aliases_with_bins),
            extra_node_paths,
        )
        .map_err(InstallWithFreshLockfileError::HoistLinkBins)?;
        // Stash the public-hoist alias list so the post-build top-level
        // bin link resolves direct-over-hoisted precedence
        // (pnpm/pacquet#342) instead of leaving it to the per-importer
        // `link_bins` walk below.
        publicly_hoisted_for_post_build = result.publicly_hoisted_aliases_with_bins;
        result.hoisted_dependencies
    } else {
        HoistedDependencies::new()
    };

    // Link bins. Direct dependencies first (each importer's
    // `node_modules/.bin`) and then per-slot children inside the virtual
    // store, using the same two-call shape as
    // `install_frozen_lockfile.rs`. One pass per importer so sibling
    // workspace projects get their own `.bin/` populated.
    let modules_basename = config
        .modules_dir
        .file_name()
        .map_or_else(|| std::ffi::OsString::from("node_modules"), std::ffi::OsStr::to_os_string);
    for importer_id in project_anchor_importer_ids {
        let project_dir =
            crate::symlink_direct_dependencies::importer_root_dir(symlink_root, importer_id);
        let modules_dir = project_dir.join(&modules_basename);
        let bins_dir = modules_dir.join(".bin");
        link_bins::<Host>(&modules_dir, &bins_dir, extra_node_paths)
            .map_err(InstallWithFreshLockfileError::LinkBins)?;
    }

    // Drive the lockfile-driven `LinkVirtualStoreBins` path: the bin
    // linker iterates `snapshots:` (no per-slot `read_dir`) and reads
    // each child's manifest from `package_manifests` (no per-child
    // `package.json` disk read on warm hits).
    //
    // The freshly-built `packages:` rows carry the same `hasBin` the
    // on-disk lockfile gets (the resolver's picked manifest keeps `bin`
    // through its flatten catch-all, and `dependencies_graph_to_lockfile`
    // reads it off), so the bin linker's `has_bin_set` short-circuit —
    // the one the frozen path trusts on the very same rows after a
    // save/load round-trip — is just as sound here.
    LinkVirtualStoreBins {
        layout,
        snapshots: materialization_lockfile.snapshots.as_ref(),
        packages: materialization_lockfile.packages.as_ref(),
        package_manifests,
        skipped,
        extra_node_paths,
    }
    .run()
    .map_err(InstallWithFreshLockfileError::LinkVirtualStoreBins)?;

    Ok(LinkPhaseOutput {
        hoisted_dependencies,
        hoisted_locations: BTreeMap::new(),
        hoisted_pkg_roots_by_key: None,
        publicly_hoisted_for_post_build,
    })
}

/// Write the module-resolution sidecars the lifecycle scripts read:
/// `node_modules/.package-map.json` and, under `nodeLinker: pnp`, the
/// `PnP` loader.
///
/// Both run before the build phase, since [`build_extra_env`] points
/// lifecycle scripts' `NODE_OPTIONS` at the package map. `layout`
/// already resolves each snapshot to its real on-disk slot (flat or
/// global-virtual-store). Reached only after materialization, mirroring
/// the frozen path's write in `InstallFrozenLockfile::run`, so the
/// `lockfile_only` early return never writes a map for an unlinked tree.
pub(super) fn write_module_resolution_sidecars(
    config: &Config,
    node_linker: pacquet_config::NodeLinker,
    materialization_lockfile: &Lockfile,
    layout: &VirtualStoreLayout,
    importer_manifests: &BTreeMap<String, &PackageManifest>,
    project_anchor_importer_ids: &HashSet<String>,
    lockfile_dir: &Path,
) -> Result<(), InstallWithFreshLockfileError> {
    let write_package_map = crate::should_write_package_map(config, node_linker);
    let write_pnp = matches!(node_linker, pacquet_config::NodeLinker::Pnp);
    if !write_package_map && !write_pnp {
        return Ok(());
    }

    let project_manifests = importer_manifests
        .iter()
        .filter(|(id, _)| project_anchor_importer_ids.contains(id.as_str()))
        .map(|(id, manifest)| (lockfile_dir.join(id), *manifest))
        .collect::<Vec<_>>();
    if write_package_map {
        crate::package_map::write_package_map(
            materialization_lockfile,
            &crate::package_map::PackageMapOptions {
                lockfile_dir,
                modules_dir: &config.modules_dir,
                package_map_type: config.node_package_map_type,
                layout,
                project_manifests: &project_manifests,
            },
        )
        .map_err(InstallWithFreshLockfileError::WritePackageMap)?;
    }
    if write_pnp {
        crate::write_pnp_file(
            materialization_lockfile,
            lockfile_dir,
            config,
            layout,
            &project_manifests,
        )
        .map_err(InstallWithFreshLockfileError::WritePnpFile)?;
    }
    Ok(())
}
