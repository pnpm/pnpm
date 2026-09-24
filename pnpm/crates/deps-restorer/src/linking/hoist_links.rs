//! Writing a hoist plan: the private and public hoisted symlinks and the
//! bins of the privately hoisted packages.

use super::LinkPhaseError;
use crate::{
    VirtualStoreLayout, install_frozen_lockfile::HoistPlan, link_direct_dep_bins_resolved,
    symlink_hoisted_dependencies,
};
use pnpm_cmd_shim::LinkBinsOptions;
use pnpm_config::Config;

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
/// Enabling the global virtual store does not move the private target:
/// pacquet leaves `virtual_store_dir` at its project-local (or
/// yaml-pinned) value and routes the shared root through
/// `global_virtual_store_dir` instead — see
/// [`Config::apply_global_virtual_store_derivation`]. Only the symlink
/// *target* under the slot dir is GVS-aware, which `layout` resolves.
pub(super) fn write_hoist_links(
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
