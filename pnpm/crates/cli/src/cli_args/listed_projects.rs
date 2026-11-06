use crate::cli_args::recursive::{
    AutoExcludeRoot, discover_workspace_projects, select_recursive_projects,
};
use miette::IntoDiagnostic;
use pnpm_config::Config;
use pnpm_package_manifest::safe_read_project_manifest_from_dir;
use pnpm_workspace_projects_graph::BaseProject;
use std::path::{Path, PathBuf};

/// The directory and manifest name of each project a report covers: the
/// `--filter` selection under `--recursive`, the project in `dir`
/// otherwise.
pub fn listed_projects(
    config: &Config,
    dir: &Path,
    recursive: bool,
) -> miette::Result<Vec<(PathBuf, Option<String>)>> {
    if !recursive {
        let name = safe_read_project_manifest_from_dir(dir)
            .into_diagnostic()?
            .and_then(|manifest| {
                manifest
                    .get("name")?
                    .as_str()
                    .map(ToString::to_string)
            });
        return Ok(vec![(dir.to_path_buf(), name)]);
    }
    let workspace_root = config.workspace_dir.as_deref().unwrap_or(dir);
    let (projects, _) = discover_workspace_projects(workspace_root, config)?;
    let selection = select_recursive_projects(&projects, config, dir, AutoExcludeRoot::Disabled)?;
    Ok(selection.selected
        .iter()
        .map(|(project_dir, project)| {
            (project_dir.clone(), project.package.manifest_name().map(ToString::to_string))
        })
        .collect())
}
