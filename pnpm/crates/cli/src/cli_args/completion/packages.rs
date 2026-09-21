use super::CompletionContext;
use pnpm_workspace::{
    FindWorkspaceProjectsOpts, find_workspace_dir, find_workspace_projects,
    read_workspace_manifest, workspace_package_patterns,
};

pub(super) fn complete_packages(context: &CompletionContext<'_>) -> miette::Result<Vec<String>> {
    let directory = context.resolve_project_directory()?;
    let workspace_dir = find_workspace_dir(&directory)?.unwrap_or(directory);
    let manifest = read_workspace_manifest(&workspace_dir)?;
    let projects = find_workspace_projects(
        &workspace_dir,
        &FindWorkspaceProjectsOpts { patterns: manifest.as_ref().map(workspace_package_patterns) },
    )?;
    let mut names: Vec<_> = projects
        .iter()
        .filter_map(|project| {
            project.manifest
                .value()
                .get("name")?
                .as_str()
        })
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}
