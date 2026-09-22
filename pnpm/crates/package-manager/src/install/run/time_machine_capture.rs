use super::{InstallScope, RunExecution};

#[cfg(target_os = "macos")]
pub(super) fn capture_time_machine_exclusions(
    execution: &RunExecution<'_>,
    scope: &InstallScope<'_>,
    exclusions: &mut super::super::TimeMachineExclusions,
) {
    let config = execution.install.context.config;
    if execution.install.execution.lockfile_only
        || execution.install.execution.dry_run
        || (config.macos_backup.modules_dir && config.macos_backup.store_dir)
    {
        return;
    }
    let project_dirs = if config.macos_backup.modules_dir {
        Vec::new()
    } else {
        // A filtered non-hoisted install can follow `link:` dependencies
        // into unselected workspace importers. Capture every possible
        // importer before the install; `apply` keeps only directories the
        // install actually created.
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
