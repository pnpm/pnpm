//! Writing a hoist plan: the private and public hoisted symlinks and the
//! bins of the privately hoisted packages.

use super::{LinkPhaseError, LinkPhaseInputs};
use crate::{
    SkippedSnapshots, VirtualStoreLayout,
    install_frozen_lockfile::{
        HoistPlan, collect_public_hoist_targets, compute_hoist_plan, workspace_packages_for_hoist,
    },
    link_direct_dep_bins_resolved, symlink_hoisted_dependencies,
};
use pnpm_cmd_shim::LinkBinsOptions;
use pnpm_config::Config;
use std::{collections::BTreeMap, path::PathBuf};

/// Planned before the links are written, not with them: an importer's
/// dep that public-hoist lands at root has to be in
/// `SymlinkDirectDependencies`'s dedupe map, and [`write_hoist_links`]
/// reuses the plan rather than walking a second time.
pub(super) struct PlannedHoist {
    pub(super) plan: Option<HoistPlan>,
    pub(super) public_targets: Option<BTreeMap<String, PathBuf>>,
}

pub(super) fn plan_hoist(inputs: &LinkPhaseInputs<'_>, skipped: &SkippedSnapshots) -> PlannedHoist {
    let config = inputs.ctx.config;
    // `hoistWorkspacePackages`: named non-root projects become hoist
    // candidates whose links point at the project dirs.
    let hoisted_workspace_packages = config.hoist_workspace_packages.then(|| {
        workspace_packages_for_hoist(
            inputs.ctx.workspace_root,
            inputs.projects.package_map_manifests,
        )
    });
    let phase_start = std::time::Instant::now();
    let plan = compute_hoist_plan(
        config,
        inputs.graph.lockfile.snapshots.as_ref(),
        inputs.graph.lockfile.packages.as_ref(),
        &inputs.graph.lockfile.importers,
        inputs.projects.dependency_groups,
        skipped,
        inputs.ctx.is_hoisted(),
        hoisted_workspace_packages.as_ref(),
    );
    let public_targets = plan
        .as_ref()
        .map(|plan| {
            collect_public_hoist_targets(
                &plan.result,
                &plan.graph,
                inputs.ctx.linker.layout,
                &plan.skipped,
            )
        });
    tracing::info!(target: "pacquet::install::phase", phase = "link.hoist_plan", elapsed_ms = phase_start.elapsed().as_millis() as u64, "phase complete");
    PlannedHoist { plan, public_targets }
}

/// Publicly hoisted *workspace* packages are the one source of root
/// bins nothing else shims: every importer's direct-dep bins were
/// written by `SymlinkDirectDependencies`, and publicly hoisted regular
/// packages go through the post-build top-level pass
/// (`publicly_hoisted_for_post_build`) — but that pass resolves bins out
/// of virtual-store slots, which a workspace project doesn't have.
/// Collected before [`write_hoist_links`] consumes the plan; shimmed after
/// the hoist symlinks land.
pub(super) fn public_workspace_bin_deps(plan: Option<&HoistPlan>) -> Vec<(String, PathBuf)> {
    plan.map(|plan| {
        plan.result.hoisted_workspace_aliases
            .iter()
            .filter(|(_, kind, _)| matches!(kind, pnpm_modules_yaml::HoistKind::Public))
            .map(|(alias, _, project_dir)| (alias.clone(), project_dir.clone()))
            .collect()
    })
    .unwrap_or_default()
}

/// What [`write_hoist_links`] put on disk.
pub(super) struct HoistLinks {
    pub(super) hoisted_dependencies: crate::HoistedDependencies,
    /// See [`super::LinkPhaseOutput::publicly_hoisted_for_post_build`].
    pub(super) publicly_hoisted_with_bins: Vec<String>,
}

impl HoistLinks {
    /// The result of a run with no hoist plan to write.
    pub(super) fn none() -> Self {
        HoistLinks {
            hoisted_dependencies: crate::HoistedDependencies::new(),
            publicly_hoisted_with_bins: Vec::new(),
        }
    }
}

/// Symlink the hoist plan's aliases into the private
/// (`<virtual_store>/node_modules`) and public (`<root>/node_modules`)
/// targets, then shim the private side's bins.
///
/// Enabling the global virtual store does not move the private target,
/// which is under [`Config::install_state_dir`]. Only the symlink
/// *target* under the slot dir is GVS-aware, which `layout` resolves.
pub(super) fn write_hoist_links(
    plan: HoistPlan,
    config: &Config,
    layout: &VirtualStoreLayout,
    link_options: &LinkBinsOptions,
) -> Result<HoistLinks, LinkPhaseError> {
    let HoistPlan { graph, result, skipped, .. } = plan;
    let private_hoist_dir = config.install_state_dir.join("node_modules");
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
