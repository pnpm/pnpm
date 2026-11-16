//! The workspace projects an install reads: walked from disk, supplied by an
//! embedder, or shared by the command across its per-project installs.

use super::{
    super::super::{InstallError, InstallRunOptions, Path, load_workspace_projects},
    InstallOwned, InstallView, WorkspaceDirs,
};

/// The workspace projects the run reads, and the workspace the command
/// discovered for all its per-project installs when they come from it.
pub(in crate::install::run) struct LoadedWorkspace {
    pub(in crate::install::run) projects: Option<std::sync::Arc<[pnpm_workspace::Project]>>,
    discovered: Option<crate::DiscoveredWorkspace>,
    /// The routes the lockfile's plain keys resolved through, as the
    /// previous install recorded them (see
    /// [`WorkspaceStateSettings::registry_routes`](pnpm_workspace_state::WorkspaceStateSettings::registry_routes)).
    /// `None` without a readable record.
    pub(in crate::install::run) locked_registry_routes:
        Option<std::collections::BTreeMap<String, String>>,
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
        let discovered = owned.projects.discovered_workspace.clone();
        let known_workspace_projects = owned.projects
            .workspace_projects_override
            .take()
            .map(std::sync::Arc::from)
            .or_else(|| {
                discovered.as_ref().map(|workspace| std::sync::Arc::clone(&workspace.projects))
            });
        let projects = discovered_workspace_projects(
            install.context.config,
            (options.selection.is_some(), known_workspace_projects),
            dirs.workspace_dir.as_deref().unwrap_or(&dirs.workspace_root),
            workspace_manifest,
        )?;
        let locked_registry_routes =
            locked_registry_routes(install.context.config, &dirs.workspace_root);
        Ok(LoadedWorkspace { projects, discovered, locked_registry_routes })
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
/// See [`LoadedWorkspace::locked_registry_routes`].
fn locked_registry_routes(
    config: &pnpm_config::Config,
    workspace_root: &Path,
) -> Option<std::collections::BTreeMap<String, String>> {
    pnpm_workspace_state::load_workspace_state_in_modules(&config.workspace_state_modules_dir(
        workspace_root,
    ))
    .ok()
    .flatten()
    .and_then(|state| state.settings.registry_routes)
}

/// Walk every workspace project's `package.json` once, unless the caller
/// supplied the list. A selection carries its own projects, so it walks
/// nothing. A walk applies [`crate::apply_runtime_on_fail`] to the manifests
/// it reads.
fn discovered_workspace_projects(
    config: &pnpm_config::Config,
    (has_selection, known_workspace_projects): (
        bool,
        Option<std::sync::Arc<[pnpm_workspace::Project]>>,
    ),
    workspace_dir: &Path,
    workspace_manifest: Option<&pnpm_workspace::WorkspaceManifest>,
) -> Result<Option<std::sync::Arc<[pnpm_workspace::Project]>>, InstallError> {
    if has_selection {
        return Ok(None);
    }
    if let Some(projects) = known_workspace_projects {
        return Ok(Some(projects));
    }
    let mut projects =
        load_workspace_projects(workspace_dir, workspace_manifest, &config.managed_directories())
            .map_err(InstallError::FindWorkspaceProjects)?;
    if let Some(projects) = &mut projects {
        crate::apply_runtime_on_fail_to_projects(config, projects);
    }
    Ok(projects.map(std::sync::Arc::from))
}
