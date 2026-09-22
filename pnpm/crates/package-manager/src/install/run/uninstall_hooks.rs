use super::{
    super::{InstallError, Reporter, mutated_project_dirs, run_pre_uninstall_scripts},
    dispatch::{Settled, SettledProjects},
};

/// Runs before the lockfile and `node_modules` are touched, so a failing
/// script leaves the project as it was.
pub(super) fn run_pre_uninstall_hooks<Reporter: self::Reporter>(
    settled: Settled<'_, '_>,
    selection: Option<&crate::WorkspaceInstallSelection<'_>>,
) -> Result<(), InstallError> {
    let Settled {
        install,
        mode,
        projects: SettledProjects { workspace, project_manifests, .. },
        ..
    } = settled;
    let config = install.context.config;
    if install.execution.mutation != crate::ProjectMutation::UninstallSome
        || config.ignore_scripts
        || mode.resolve_only
    {
        return Ok(());
    }
    let mutated_dirs = mutated_project_dirs(
        workspace.dirs.manifest_dir,
        selection.map(|selection| selection.selected_dirs),
        selection.and_then(|selection| selection.edited_dirs),
    );
    let projects = project_manifests
        .iter()
        .filter(|(project_dir, _)| mutated_dirs.contains(&pnpm_fs::lexical_normalize(project_dir)))
        .cloned()
        .collect::<Vec<_>>();
    run_pre_uninstall_scripts::<Reporter>(
        config,
        install.execution.node_linker,
        &workspace.dirs.workspace_root,
        &projects,
    )
}
