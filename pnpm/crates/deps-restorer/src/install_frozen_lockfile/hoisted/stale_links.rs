//! Links a previous install wrote that this one no longer wants, removed
//! before the linker writes the tree.

use super::{HoistedLinkerError, HoistedLinkerInputs, workspace_links::ALL_DEPENDENCY_GROUPS};
use crate::{SkippedSnapshots, prune_stale_modules::ReportedRemovals};
use pnpm_reporter::Reporter;

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
pub(super) fn unlink_stale_direct_links<Reporter: self::Reporter>(
    inputs: &HoistedLinkerInputs<'_>,
    skipped: &SkippedSnapshots,
) -> Result<(), HoistedLinkerError> {
    let Some(current_lockfile) = inputs.prior.current_lockfile else { return Ok(()) };
    crate::prune_stale_modules::StaleDirectDepLinks {
        workspace_root: inputs.projects.symlink_workspace_root,
        modules_dir_name: inputs.config.modules_dir_name(),
        wanted_lockfile: inputs.graph.lockfile,
        current_lockfile,
        linked: crate::LinkedDirectDeps { included_groups: &ALL_DEPENDENCY_GROUPS, skipped },
        dedupe_direct_deps: inputs.config.dedupe_direct_deps,
        reported: ReportedRemovals::UnlinkedLinkDependencies,
    }
    .unlink::<Reporter>()
    .map_err(HoistedLinkerError::PruneStaleLinks)
}
