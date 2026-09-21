use super::{InstallScope, RunExecution};

#[cfg(target_os = "macos")]
pub(super) fn capture_time_machine_exclusions(
    execution: &RunExecution<'_>,
    scope: &InstallScope<'_>,
    exclusions: &mut super::super::TimeMachineExclusions,
) {
    let config = execution.install.context.config;
    if config.macos_backup.modules_dir && config.macos_backup.store_dir {
        return;
    }
    let project_dirs = if config.macos_backup.modules_dir {
        Vec::new()
    } else if let Some(selection) = execution.options.selection.as_ref() {
        selection.install_dirs
            .iter()
            .cloned()
            .collect()
    } else {
        scope.project_manifests
            .iter()
            .map(|(dir, _)| dir.clone())
            .collect()
    };
    *exclusions = super::super::TimeMachineExclusions::capture(
        config,
        execution.install.execution,
        &execution.workspace.dirs.workspace_root,
        &project_dirs,
    );
}

#[cfg(not(target_os = "macos"))]
pub(super) fn capture_time_machine_exclusions(
    _: &RunExecution<'_>,
    _: &InstallScope<'_>,
    exclusions: &mut super::super::TimeMachineExclusions,
) {
    *exclusions = super::super::TimeMachineExclusions::empty();
}
