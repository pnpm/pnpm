//! Build the workspace dependency graphs a recursive run filters and sorts through.

use crate::cli_args::catalogs::{configured_catalogs, workspace_catalogs};
use miette::{Context, IntoDiagnostic};
use pnpm_config::{Config, LinkWorkspacePackages};
use pnpm_package_manager::overrides_dependency_rewriter;
use pnpm_workspace::{GraphPkg, Project};
use pnpm_workspace_projects_graph::{
    CreateProjectsGraphOptions, DependencyRewriter, ProjectGraph, create_projects_graph,
};
use std::path::Path;

/// The graph a recursive run resolves selection and order through, with
/// the prod-pruned twin a `--filter-prod` selector is matched against.
pub(super) struct FilterGraphs<'a> {
    pub(super) all: ProjectGraph<GraphPkg<'a>>,
    pub(super) prod_all: Option<ProjectGraph<GraphPkg<'a>>>,
}

/// Build the graphs [`super::select_recursive_projects`] filters and sorts through.
///
/// They follow the configured `link-workspace-packages` policy. Under the
/// default `link-workspace-packages: false` a bare-semver range naming a
/// sibling is not a workspace edge, so it drives neither selection nor
/// order; only a `workspace:` range, an enabled policy, or an override
/// pointing the dependency at the sibling links it.
pub(super) fn build_filter_graphs<'a>(
    projects: &'a [Project],
    config: &Config,
    prefix: &Path,
) -> miette::Result<FilterGraphs<'a>> {
    let workspace_dir = config.workspace_dir.as_deref().unwrap_or(prefix);
    let catalogs = configured_catalogs(config)?;
    let dependency_rewriter = overrides_dependency_rewriter(config, &catalogs, workspace_dir)
        .into_diagnostic()
        .wrap_err("parsing the overrides")?;
    let options = CreateProjectsGraphOptions {
        link_workspace_packages: Some(config.link_workspace_packages != LinkWorkspacePackages::Off),
        catalogs: workspace_catalogs(config, &catalogs),
        dependency_rewriter: dependency_rewriter
            .as_ref()
            .map(|rewriter| rewriter as &dyn DependencyRewriter),
        ..CreateProjectsGraphOptions::default()
    };
    Ok(FilterGraphs {
        all: build_graph(projects, options),
        prod_all: (!config.filter_prod.is_empty()).then(|| {
            build_graph(projects, CreateProjectsGraphOptions { ignore_dev_deps: true, ..options })
        }),
    })
}

/// Build the workspace [`ProjectGraph`] from `projects` under `options`.
fn build_graph<'p>(
    projects: &'p [Project],
    options: CreateProjectsGraphOptions<'_>,
) -> ProjectGraph<GraphPkg<'p>> {
    create_projects_graph(
        projects
            .iter()
            .map(|project| GraphPkg { project })
            .collect(),
        &options,
    )
    .graph
}
