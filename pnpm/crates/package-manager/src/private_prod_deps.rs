//! Publishable workspace projects that depend on private ones.
//!
//! `dependencies` is the group a published package asks its consumers to
//! install. A private workspace project is not on the registry, so that
//! install cannot succeed. `devDependencies` stay allowed: they are how a
//! publishable project uses a private package while it is built.

use pnpm_catalogs_types::Catalogs;
use pnpm_config::{Config, LinkWorkspacePackages};
use pnpm_workspace::{GraphPkg, Project};
use pnpm_workspace_projects_graph::{
    CreateProjectsGraphOptions, ProjectGraph, WorkspaceCatalogs, create_projects_graph,
};
use std::path::Path;

/// Whether a repeat-install shortcut must not claim the workspace is
/// already up to date. `projects` is `None` when this shortcut does not
/// have the project list; the full install then runs the check itself.
pub(crate) fn private_prod_deps_block_short_circuit(
    config: &Config,
    workspace_dir: Option<&Path>,
    projects: Option<&[Project]>,
    catalogs: &Catalogs,
) -> bool {
    if !config.disallow_private_prod_deps {
        return false;
    }
    let Some(workspace_dir) = workspace_dir else { return false };
    let Some(projects) = projects else { return true };
    !private_workspace_prod_deps(config, workspace_dir, projects, catalogs).is_empty()
}

/// Pairs of `(publishable, private)` package labels. Empty when the
/// setting is off, or when no publishable project lists a private
/// workspace project in `dependencies`.
pub(crate) fn private_workspace_prod_deps<'a>(
    config: &Config,
    workspace_dir: &Path,
    projects: &'a [Project],
    catalogs: &'a Catalogs,
) -> Vec<(String, String)> {
    if !config.disallow_private_prod_deps || projects.len() < 2 {
        return Vec::new();
    }
    let catalogs = WorkspaceCatalogs { catalogs, workspace_dir };
    let graph = production_graph(config, projects, Some(catalogs));
    violations(&graph)
}

pub(crate) fn render_private_prod_deps(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(from, to)| format!("{from} depends on private workspace package {to}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn production_graph<'a>(
    config: &Config,
    projects: &'a [Project],
    catalogs: Option<WorkspaceCatalogs<'a>>,
) -> ProjectGraph<GraphPkg<'a>> {
    create_projects_graph(
        projects
            .iter()
            .map(|project| GraphPkg { project })
            .collect(),
        &CreateProjectsGraphOptions {
            production_only: true,
            link_workspace_packages: Some(
                config.link_workspace_packages != LinkWorkspacePackages::Off,
            ),
            catalogs,
            ..CreateProjectsGraphOptions::default()
        },
    )
    .graph
}

fn violations(graph: &ProjectGraph<GraphPkg<'_>>) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for node in graph.values() {
        if project_is_private(node.package.project) {
            continue;
        }
        let from = project_label(node.package.project);
        for target_dir in &node.dependencies {
            let Some(target) = graph.get(target_dir) else { continue };
            if !project_is_private(target.package.project) {
                continue;
            }
            pairs.push((from.clone(), project_label(target.package.project)));
        }
    }
    pairs.sort();
    pairs
}

fn project_is_private(project: &Project) -> bool {
    project.manifest
        .value()
        .get("private")
        .and_then(|value| value.as_bool())
        == Some(true)
}

fn project_label(project: &Project) -> String {
    project.manifest
        .value()
        .get("name")
        .and_then(|name| name.as_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| project.root_dir.display().to_string())
}

#[derive(Debug, derive_more::Display, derive_more::Error, miette::Diagnostic)]
#[diagnostic(
    code(ERR_PNPM_PRIVATE_WORKSPACE_PROD_DEP),
    help("Move the dependency to devDependencies, or stop marking the dependency private.")
)]
pub struct PrivateWorkspaceProdDepError {
    #[error(not(source))]
    pub message: String,
}
