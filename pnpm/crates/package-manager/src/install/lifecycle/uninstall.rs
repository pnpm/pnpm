use super::{
    super::{
        Config, InstallError, NodeLinker, PROJECT_LIFECYCLE_STAGES, PROJECT_POST_UNINSTALL_STAGES,
        PROJECT_PRE_UNINSTALL_STAGES, PackageManifest, Path, PathBuf, ProjectMutation, Reporter,
        project_requires_lifecycle_scripts,
    },
    ProjectScriptRunner,
};

/// The stages a project runs once this run has materialized it.
pub(in crate::install) fn project_script_stages(
    mutation: ProjectMutation,
) -> &'static [&'static str] {
    match mutation {
        ProjectMutation::UninstallSome => &PROJECT_POST_UNINSTALL_STAGES,
        ProjectMutation::InstallWorkspace
        | ProjectMutation::InstallSelected
        | ProjectMutation::InstallSome
        | ProjectMutation::NoInstall => &PROJECT_LIFECYCLE_STAGES,
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
            runner.run::<Reporter>(project_dir, manifest, &PROJECT_PRE_UNINSTALL_STAGES)?;
        }
    }
    Ok(())
}
