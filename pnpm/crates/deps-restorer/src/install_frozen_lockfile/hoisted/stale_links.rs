//! Links a previous install wrote that this one no longer wants, removed
//! before the linker writes the tree.

use super::{HoistedLinkerError, HoistedLinkerInputs};

/// Runs before the linker writes the tree, so no package is imported at
/// a path that is still a project's link.
pub(super) fn unlink_prior_workspace_hoists(
    inputs: &HoistedLinkerInputs<'_>,
) -> Result<(), HoistedLinkerError> {
    let modules_dir = &inputs.config.modules_dir;
    crate::prune_stale_modules::prune_workspace_hoists(
        &crate::prune_stale_modules::WorkspaceHoistDirs {
            workspace_root: inputs.projects.walker_lockfile_dir,
            private: modules_dir,
            public: modules_dir,
        },
        inputs.prior.current_lockfile.unwrap_or(inputs.graph.lockfile),
        inputs.prior.hoisted_dependencies,
        None,
    )
    .map_err(HoistedLinkerError::PruneStaleLinks)
}

/// Runs before the linker writes the tree, so a package that replaces a
/// removed `link:` dependency is not imported through the old link.
pub(super) fn unlink_stale_direct_links(
    inputs: &HoistedLinkerInputs<'_>,
) -> Result<(), HoistedLinkerError> {
    let Some(current_lockfile) = inputs.prior.current_lockfile else { return Ok(()) };
    crate::prune_stale_modules::prune_stale_hoisted_direct_links(
        inputs.projects.symlink_workspace_root,
        inputs.config.modules_dir_name(),
        inputs.graph.lockfile,
        current_lockfile,
    )
    .map_err(HoistedLinkerError::PruneStaleLinks)
}
