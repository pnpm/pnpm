use super::{
    InstallScope,
    RunExecution,
};
#[cfg(target_os = "macos")]
use indexmap::IndexMap;
#[cfg(target_os = "macos")]
use pnpm_config::LinkWorkspacePackages;
#[cfg(target_os = "macos")]
use pnpm_package_manifest::{
    DependencyGroup,
    PackageManifest,
};
#[cfg(target_os = "macos")]
use pnpm_workspace_projects_graph::{
    BaseProject,
    CreateProjectsGraphOptions,
    GraphProject,
    ProjectGraph,
    create_projects_graph,
};
#[cfg(target_os = "macos")]
use std::{
    collections::HashSet,
    path::{
        Path,
        PathBuf,
    },
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
        || (!config.macos_backup.exclude_modules_dir && !config.macos_backup.exclude_store_dir)
    {
        return;
    }
    let project_dirs = if config.macos_backup.exclude_modules_dir {
        // A filtered non-hoisted install can follow dependency edges into
        // unselected workspace importers, including edges introduced by
        // package hooks. Capture every workspace root before the install;
        // `apply` keeps only directories the install actually created.
        project_dirs_to_capture(
            scope,
            None,
            config.link_workspace_packages != LinkWorkspacePackages::Off,
        )
    } else {
        Vec::new()
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
    link_workspace_packages: bool,
) -> Vec<PathBuf> {
    let Some(selected_dirs) = selected_dirs else {
        return scope.project_manifests
            .iter()
            .map(|(dir, _)| dir.clone())
            .collect();
    };
    let graph = capture_projects_graph(scope, link_workspace_packages);
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
        for target in linked_workspace_dirs(&graph, &project_dir) {
            if seen.insert(target.clone()) {
                project_dirs.push(target);
            }
        }
        index += 1;
    }
    project_dirs
}

#[cfg(target_os = "macos")]
fn linked_workspace_dirs(
    graph: &ProjectGraph<CaptureProject<'_>>,
    project_dir: &Path,
) -> Vec<PathBuf> {
    let normalized_project_dir = pnpm_fs::lexical_normalize(project_dir);
    graph
        .iter()
        .find_map(|(dir, node)| {
            (pnpm_fs::lexical_normalize(dir) == normalized_project_dir).then(|| {
                node.dependencies.clone()
            })
        })
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn capture_projects_graph<'a>(
    scope: &'a InstallScope<'a>,
    link_workspace_packages: bool,
) -> ProjectGraph<CaptureProject<'a>> {
    create_projects_graph(
        scope.project_manifests
            .iter()
            .map(|(dir, manifest)| CaptureProject { dir, manifest })
            .collect(),
        &CreateProjectsGraphOptions {
            link_workspace_packages: Some(link_workspace_packages),
            ..CreateProjectsGraphOptions::default()
        },
    )
    .graph
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
struct CaptureProject<'a> {
    dir: &'a Path,
    manifest: &'a PackageManifest,
}

#[cfg(target_os = "macos")]
impl BaseProject for CaptureProject<'_> {
    fn root_dir(&self) -> &Path {
        self.dir
    }

    fn manifest_name(&self) -> Option<&str> {
        self.manifest
            .value()
            .get("name")
            .and_then(|name| name.as_str())
    }
}

#[cfg(target_os = "macos")]
impl GraphProject for CaptureProject<'_> {
    fn manifest_version(&self) -> Option<&str> {
        self.manifest
            .value()
            .get("version")
            .and_then(|version| version.as_str())
    }

    fn merged_dependencies(&self, _: bool) -> Vec<(String, String)> {
        let mut dependencies = IndexMap::new();
        for (name, spec) in self.manifest.dependencies([
            DependencyGroup::Peer,
            DependencyGroup::Dev,
            DependencyGroup::Optional,
            DependencyGroup::Prod,
        ]) {
            dependencies.insert(name.to_string(), spec.to_string());
        }
        dependencies.into_iter().collect()
    }
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
