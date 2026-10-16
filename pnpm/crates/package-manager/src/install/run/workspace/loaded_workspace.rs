//! The workspace projects an install reads: walked from disk, supplied by an
//! embedder, or shared by the command across its per-project installs.

use super::{
    super::super::{InstallError, InstallRunOptions, Path, PathBuf, load_workspace_projects},
    InstallOwned, InstallView, WorkspaceDirs,
};

/// The workspace projects the run reads, and the workspace the command
/// discovered for all its per-project installs when they come from it.
pub(in crate::install::run) struct LoadedWorkspace {
    pub(in crate::install::run) projects: Option<std::sync::Arc<[pnpm_workspace::Project]>>,
    discovered: Option<crate::DiscoveredWorkspace>,
}
impl LoadedWorkspace {
    pub(super) fn load(
        install: InstallView<'_>,
        owned: &mut InstallOwned,
        options: &InstallRunOptions<'_, '_>,
        (dirs, workspace_manifest): (
            &WorkspaceDirs<'_>,
            Option<&pnpm_workspace::WorkspaceManifest>,
        ),
    ) -> Result<Self, InstallError> {
        let discovered = owned.projects.dedicated
            .as_ref()
            .and_then(|dedicated| dedicated.workspace.clone());
        let known_workspace_projects = owned.projects
            .workspace_projects_override
            .take()
            .map(std::sync::Arc::from)
            .or_else(|| {
                discovered.as_ref().map(|workspace| std::sync::Arc::clone(&workspace.projects))
            });
        let projects = discovered_workspace_projects(
            options.selection.is_some(),
            known_workspace_projects,
            dirs.workspace_dir.as_deref().unwrap_or(&dirs.workspace_root),
            workspace_manifest,
            &install.context.config.managed_directories(),
        )?;
        Ok(LoadedWorkspace { projects, discovered })
    }

    /// The command's workspace-package index, when `projects` are the
    /// command's own list. See [`crate::DiscoveredWorkspace`].
    pub(in crate::install::run) fn packages_of(
        &self,
        projects: Option<&[pnpm_workspace::Project]>,
    ) -> Option<std::sync::Arc<pnpm_resolving_resolver_base::WorkspacePackages>> {
        self.discovered.as_ref()?.packages_of(projects)
    }
}
/// Walk every workspace project's `package.json` once, unless the caller
/// supplied the list. A selection carries its own projects, so it walks
/// nothing.
fn discovered_workspace_projects(
    has_selection: bool,
    known_workspace_projects: Option<std::sync::Arc<[pnpm_workspace::Project]>>,
    workspace_dir: &Path,
    workspace_manifest: Option<&pnpm_workspace::WorkspaceManifest>,
    ignored_directories: &[PathBuf],
) -> Result<Option<std::sync::Arc<[pnpm_workspace::Project]>>, InstallError> {
    if has_selection {
        return Ok(None);
    }
    if let Some(projects) = known_workspace_projects {
        return Ok(Some(projects));
    }
    load_workspace_projects(workspace_dir, workspace_manifest, ignored_directories)
        .map(|projects| projects.map(std::sync::Arc::from))
        .map_err(InstallError::FindWorkspaceProjects)
}
