use super::{
    super::{
        Config, InstallError, NodeLinker, PROJECT_INSTALL_STAGES, PROJECT_LIFECYCLE_STAGES,
        PROJECT_POST_UNINSTALL_STAGES, PROJECT_PRE_UNINSTALL_STAGES, PackageManifest, Path,
        PathBuf, ProjectMutation, Reporter, project_requires_lifecycle_scripts,
    },
    ProjectScriptRunner,
};

/// The stages a project runs once this run has materialized it.
pub(in crate::install) fn project_script_stages(
    mutation: ProjectMutation,
    include_dev: bool,
) -> &'static [&'static str] {
    match mutation {
        ProjectMutation::UninstallSome => &PROJECT_POST_UNINSTALL_STAGES,
        ProjectMutation::InstallSome => &PROJECT_INSTALL_STAGES,
        ProjectMutation::InstallWorkspace
        | ProjectMutation::InstallSelected
        | ProjectMutation::NoInstall => {
            if include_dev {
                &PROJECT_LIFECYCLE_STAGES
            } else {
                &PROJECT_INSTALL_STAGES
            }
        }
    }
}

/// Run [`PROJECT_PRE_UNINSTALL_STAGES`] in `projects`, in order.
pub(in crate::install) fn run_pre_uninstall_scripts<Reporter: self::Reporter>(
    config: &Config,
    node_linker: NodeLinker,
    workspace_root: &Path,
    projects: &[(PathBuf, &PackageManifest)],
) -> Result<(), InstallError> {
    let runner = ProjectScriptRunner::new(config, node_linker, workspace_root, false);
    for (project_dir, manifest) in projects {
        if project_requires_lifecycle_scripts(project_dir, manifest, &PROJECT_PRE_UNINSTALL_STAGES)
        {
            runner.run_without_bin_linking::<Reporter>(project_dir, &PROJECT_PRE_UNINSTALL_STAGES)?;
        }
    }
    Ok(())
}
