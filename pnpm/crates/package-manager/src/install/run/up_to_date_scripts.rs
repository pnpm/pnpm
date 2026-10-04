use super::{
    super::{
        InstallError, LogEvent, LogLevel, PreparedModulesState, RebuildOptions, Reporter,
        SummaryLog,
    },
    Dispatched,
    dispatch::{Decided, Settled, SettledProjects},
    execution::wait_for_workspace_dependencies,
    manifests::installed_project_manifests,
};
use crate::{
    WorkspaceInstallSelection,
    install::{
        apply_materialization::completion::{
            MaterializedProjectScriptsInputs, run_materialized_project_scripts,
        },
        state_options::ProjectScriptSelection,
    },
};

/// The parts of the run options that pick which projects run scripts.
/// The rest is not `Sync`, so it cannot be held across an `.await`.
pub(super) type ScriptOptions<'a, 'selection> =
    (Option<&'a WorkspaceInstallSelection<'selection>>, Option<&'a RebuildOptions>);

/// The prepared modules state to materialize, or `None` once an install whose
/// tree was found up to date is closed. Unless the repeat-install check found
/// nothing changed, the projects still run their own lifecycle scripts, as they
/// do after a materializing install.
pub(super) async fn finish_prepared<'install, Reporter: self::Reporter>(
    settled: Settled<'_, '_>,
    options: ScriptOptions<'_, '_>,
    decided: Decided,
    prepared: Option<PreparedModulesState<'install>>,
) -> Result<Option<Dispatched<'install>>, InstallError> {
    if let Some(modules) = prepared {
        return Ok(Some(decided.with_modules(modules)));
    }
    if !settled.projects.scope.project_scripts_current {
        let dedicated = settled.owned.projects.dedicated.as_ref();
        wait_for_workspace_dependencies(dedicated.and_then(|dedicated| {
            dedicated.dependencies_installed.clone()
        }))
        .await?;
        run_up_to_date_project_scripts::<Reporter>(settled, options, decided.root_preinstall_ran)?;
    }
    Reporter::emit(&LogEvent::Summary(SummaryLog {
        level: LogLevel::Debug,
        prefix: settled.projects.workspace.prefix.clone(),
    }));
    Ok(None)
}

fn run_up_to_date_project_scripts<Reporter: self::Reporter>(
    settled: Settled<'_, '_>,
    (selection, rebuild): ScriptOptions<'_, '_>,
    root_preinstall_ran: bool,
) -> Result<(), InstallError> {
    let Settled {
        install,
        mode,
        loaded,
        projects: SettledProjects { workspace, project_manifests, .. },
        ..
    } = settled;
    let installed_project_manifests = installed_project_manifests(project_manifests, selection);
    run_materialized_project_scripts::<Reporter>(MaterializedProjectScriptsInputs {
        request: ProjectScriptSelection {
            mutation: install.execution.mutation,
            manifest_dir: workspace.dirs.manifest_dir,
            workspace: selection,
            rebuild,
            include_dev: mode.included.dev_dependencies,
        },
        config: install.context.config,
        node_linker: install.execution.node_linker,
        workspace_root: &workspace.dirs.workspace_root,
        project_manifests,
        materialized_project_manifests: &installed_project_manifests,
        materialized_current_lockfile: loaded.current.as_ref(),
        root_preinstall_ran,
    })
}
