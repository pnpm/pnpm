//! Build the workspace dependency graphs a recursive run filters and sorts through.

use crate::cli_args::catalogs::{configured_catalogs, workspace_catalogs};
use miette::{Context, IntoDiagnostic};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::{Config, LinkWorkspacePackages};
use pnpm_package_manager::{
    VersionsOverrider, overrides_catalog_reference_rewriter, overrides_dependency_rewriter,
};
use pnpm_workspace::{GraphPkg, Project};
use pnpm_workspace_projects_filter::{
    FilterWorkspaceProjectsOptions, ProjectDependencies, ProjectSelector,
};
use pnpm_workspace_projects_graph::{
    CreateProjectsGraphOptions, DependencyRewriter, ProjectGraph, create_projects_graph,
    rewritten_dependencies,
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
    let catalogs = configured_catalogs(config)?;
    let dependency_rewriter = configured_overrides_rewriter(config, &catalogs, prefix)?;
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

/// The [`overrides_dependency_rewriter`] of the workspace that `prefix` is in.
pub(super) fn configured_overrides_rewriter(
    config: &Config,
    catalogs: &Catalogs,
    prefix: &Path,
) -> miette::Result<Option<VersionsOverrider>> {
    let workspace_dir = config.workspace_dir.as_deref().unwrap_or(prefix);
    overrides_dependency_rewriter(config, catalogs, workspace_dir)
        .into_diagnostic()
        .wrap_err("parsing the overrides")
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

/// [`super::recursive_filter_options`] for evaluating `selectors` against
/// `graph`. A `[<since>]` selector among them reads the dependencies from
/// [`overridden_project_dependencies`].
pub(super) fn selector_filter_options(
    graph: &ProjectGraph<GraphPkg<'_>>,
    selectors: &[ProjectSelector],
    config: &Config,
    prefix: &Path,
) -> miette::Result<FilterWorkspaceProjectsOptions> {
    let mut options = super::recursive_filter_options(config, prefix);
    if selectors.iter().any(|selector| selector.diff.is_some()) {
        let workspace_dir = config.workspace_dir.as_deref().unwrap_or(prefix);
        options.project_dependencies =
            overridden_project_dependencies(graph, config, workspace_dir)?;
    }
    Ok(options)
}

/// Each project's dependencies with the overrides applied, keyed by root
/// directory, for finding the projects that use a changed catalog entry.
/// `None` when no overrides are configured.
pub(crate) fn overridden_project_dependencies(
    graph: &ProjectGraph<GraphPkg<'_>>,
    config: &Config,
    workspace_dir: &Path,
) -> miette::Result<Option<ProjectDependencies>> {
    let catalogs = configured_catalogs(config)?;
    let Some(rewriter) = overrides_catalog_reference_rewriter(config, &catalogs, workspace_dir)
        .into_diagnostic()
        .wrap_err("parsing the overrides")?
    else {
        return Ok(None);
    };
    Ok(Some(
        graph
            .iter()
            .map(|(dir, node)| {
                (dir.clone(), rewritten_dependencies(&node.package, false, Some(&rewriter)))
            })
            .collect(),
    ))
}
