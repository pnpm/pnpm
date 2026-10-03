use super::{
    super::{
        InstallError, InstallRunOptions, LogEvent, LogLevel, PreparedModulesState, Reporter,
        SummaryLog,
    },
    Dispatched,
    dispatch::{Decided, Settled, SettledProjects},
    manifests::installed_project_manifests,
};
use crate::install::{
    apply_materialization::completion::{
        MaterializedProjectScriptsInputs, run_materialized_project_scripts,
    },
    state_options::ProjectScriptSelection,
};

/// The prepared modules state to materialize, or `None` once an install whose
/// tree was found up to date is closed. Unless the repeat-install check found
/// nothing changed, the projects still run their own lifecycle scripts, as they
/// do after a materializing install.
pub(super) fn finish_prepared<'install, Reporter: self::Reporter>(
    settled: Settled<'_, '_>,
    options: &InstallRunOptions<'_, '_>,
    decided: Decided,
    prepared: Option<PreparedModulesState<'install>>,
) -> Result<Option<Dispatched<'install>>, InstallError> {
    if let Some(modules) = prepared {
        return Ok(Some(decided.with_modules(modules)));
    }
    if !settled.projects.scope.project_scripts_current {
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
    options: &InstallRunOptions<'_, '_>,
    root_preinstall_ran: bool,
) -> Result<(), InstallError> {
    let Settled {
        install,
        mode,
        loaded,
        projects: SettledProjects { workspace, project_manifests, .. },
        ..
    } = settled;
    let selection = options.selection.as_ref();
    let installed_project_manifests = installed_project_manifests(project_manifests, selection);
    run_materialized_project_scripts::<Reporter>(MaterializedProjectScriptsInputs {
        request: ProjectScriptSelection {
            mutation: install.execution.mutation,
            manifest_dir: workspace.dirs.manifest_dir,
            workspace: selection,
            rebuild: options.rebuild.as_ref(),
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
