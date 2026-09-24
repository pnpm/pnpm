//! Which lockfile importers a lockfile-driven command (`audit`, `sbom`)
//! covers under `--filter`, `--filter-prod`, or `--workspace-root`.

use super::{
    AutoExcludeRoot, discover_workspace_projects, select_recursive_projects, selected_importer_ids,
};
use pnpm_config::Config;
use std::{collections::HashSet, path::Path};

/// Whether the run asked for a subset of the workspace: any `--filter` /
/// `--filter-prod` selector, or `--workspace-root`. Without one, a
/// lockfile-driven command covers every importer in the lockfile.
pub fn selectors_narrow_the_run(config: &Config) -> bool {
    !config.filter.is_empty() || !config.filter_prod.is_empty() || config.workspace_root
}

/// The lockfile importer ids of the workspace projects the run's selectors
/// selected, for a command invoked in `project_dir` whose lockfile lives in
/// `lockfile_dir`.
pub fn selected_workspace_importer_ids(
    config: &Config,
    project_dir: &Path,
    lockfile_dir: &Path,
) -> miette::Result<HashSet<String>> {
    let workspace_root = config.workspace_dir.as_deref().unwrap_or(project_dir);
    let (projects, _) = discover_workspace_projects(workspace_root, config)?;
    let selection =
        select_recursive_projects(&projects, config, project_dir, AutoExcludeRoot::Disabled)?;
    Ok(selected_importer_ids(&selection, lockfile_dir).into_iter().collect())
}
