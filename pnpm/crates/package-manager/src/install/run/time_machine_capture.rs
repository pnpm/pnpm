use super::{InstallScope, RunExecution};
#[cfg(target_os = "macos")]
use pnpm_package_manifest::DependencyGroup;
#[cfg(target_os = "macos")]
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

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
        // into unselected workspace importers. Capture the selected importers
        // and that transitive closure before the install; `apply` keeps only
        // directories the install actually created.
        project_dirs_to_capture(
            scope,
            execution.options.selection.as_ref().map(|selection| selection.install_dirs),
        )
    };
    *exclusions = super::super::TimeMachineExclusions::capture(
        config,
        execution.install.execution,
        &execution.workspace.dirs.workspace_root,
        &project_dirs,
    );
}

#[cfg(target_os = "macos")]
fn project_dirs_to_capture(
    scope: &InstallScope<'_>,
    selected_dirs: Option<&HashSet<PathBuf>>,
) -> Vec<PathBuf> {
    let Some(selected_dirs) = selected_dirs else {
        return scope.project_manifests
            .iter()
            .map(|(dir, _)| dir.clone())
            .collect();
    };
    let mut project_dirs = selected_dirs
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let mut seen = project_dirs
        .iter()
        .map(|dir| pnpm_fs::lexical_normalize(dir))
        .collect::<HashSet<_>>();
    let mut index = 0;
    while index < project_dirs.len() {
        let project_dir = project_dirs[index].clone();
        for target in linked_workspace_dirs(scope, &project_dir) {
            if seen.insert(target.clone()) {
                project_dirs.push(target);
            }
        }
        index += 1;
    }
    project_dirs
}

#[cfg(target_os = "macos")]
fn linked_workspace_dirs(scope: &InstallScope<'_>, project_dir: &Path) -> Vec<PathBuf> {
    let normalized_project_dir = pnpm_fs::lexical_normalize(project_dir);
    let Some((_, manifest)) = scope.project_manifests
        .iter()
        .find(|(dir, _)| pnpm_fs::lexical_normalize(dir) == normalized_project_dir)
    else {
        return Vec::new();
    };
    manifest
        .dependencies([DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional])
        .filter_map(|(_, spec)| spec.strip_prefix("link:"))
        .map(|target| pnpm_fs::lexical_normalize(&project_dir.join(target)))
        .filter(|target| {
            scope.project_manifests
                .iter()
                .any(|(dir, _)| pnpm_fs::lexical_normalize(dir) == *target)
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
pub(super) fn capture_time_machine_exclusions(
    _: &RunExecution<'_>,
    _: &InstallScope<'_>,
    exclusions: &mut super::super::TimeMachineExclusions,
) {
    *exclusions = super::super::TimeMachineExclusions::empty();
}

#[cfg(all(test, target_os = "macos"))]
#[path = "time_machine_capture/tests.rs"]
mod tests;
