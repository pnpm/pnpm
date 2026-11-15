use super::{
    FilterError, FilterProjectsOptions, FilterWorkspaceProjectsOptions, FilteredProjects,
    WorkspaceFilter, filter_workspace_projects_with_prod_graph,
};
use crate::parse_project_selector::{ProjectSelector, parse_project_selector};
use pnpm_workspace_projects_graph::{
    CreateProjectsGraphOptions, GraphProject, WorkspaceCatalogs, create_projects_graph,
};

/// Parse a list of [`WorkspaceFilter`]s into selectors and apply them
/// against `projects` via [`filter_projects_by_selector_objects`].
pub fn filter_projects<Pkg>(
    projects: Vec<Pkg>,
    filter: &[WorkspaceFilter],
    opts: &FilterProjectsOptions,
) -> Result<FilteredProjects, FilterError>
where
    Pkg: GraphProject + Clone,
{
    let selectors: Vec<ProjectSelector> = filter
        .iter()
        .map(|entry| {
            let mut selector = parse_project_selector(&entry.filter, &opts.prefix);
            selector.follow_prod_deps_only = entry.follow_prod_deps_only;
            selector
        })
        .collect();
    filter_projects_by_selector_objects(projects, &selectors, opts)
}

/// Build the project graph and apply parsed `selectors`, running the
/// `--filter-prod` selectors against a production-only graph and the rest
/// against the full graph in one polarity-chunked pass.
pub fn filter_projects_by_selector_objects<Pkg>(
    mut projects: Vec<Pkg>,
    selectors: &[ProjectSelector],
    opts: &FilterProjectsOptions,
) -> Result<FilteredProjects, FilterError>
where
    Pkg: GraphProject + Clone,
{
    if selectors.is_empty() {
        return Ok(select_all_projects(projects, opts));
    }

    let has_prod = selectors.iter().any(|selector| selector.follow_prod_deps_only);
    let has_all = selectors.iter().any(|selector| !selector.follow_prod_deps_only);
    let walk_opts = workspace_filter_options(opts);

    let prod_graph = has_prod.then(|| {
        let prod_projects = if has_all { projects.clone() } else { std::mem::take(&mut projects) };
        create_projects_graph(
            prod_projects,
            &CreateProjectsGraphOptions { ignore_dev_deps: true, ..graph_options(opts) },
        )
        .graph
    });
    let all_graph = has_all.then(|| create_projects_graph(projects, &graph_options(opts)).graph);
    let primary_graph = all_graph
        .as_ref()
        .or(prod_graph.as_ref())
        .expect("non-empty selectors require at least one graph");

    let result = filter_workspace_projects_with_prod_graph(
        primary_graph,
        prod_graph.as_ref(),
        selectors,
        &walk_opts,
    )?;
    Ok(FilteredProjects {
        selected_projects: result.selected_projects,
        unmatched_filters: result.unmatched_filters,
    })
}

fn graph_options(opts: &FilterProjectsOptions) -> CreateProjectsGraphOptions<'_> {
    CreateProjectsGraphOptions {
        link_workspace_packages: opts.link_workspace_packages,
        catalogs: opts.catalogs
            .as_ref()
            .map(|catalogs| WorkspaceCatalogs { catalogs, workspace_dir: &opts.workspace_dir }),
        ..CreateProjectsGraphOptions::default()
    }
}

fn workspace_filter_options(opts: &FilterProjectsOptions) -> FilterWorkspaceProjectsOptions {
    FilterWorkspaceProjectsOptions {
        use_glob_dir_filtering: opts.use_glob_dir_filtering,
        workspace_dir: opts.workspace_dir.clone(),
        test_pattern: opts.test_pattern.clone(),
        changed_files_ignore_pattern: opts.changed_files_ignore_pattern.clone(),
    }
}

fn select_all_projects<Pkg: GraphProject + Clone>(
    projects: Vec<Pkg>,
    opts: &FilterProjectsOptions,
) -> FilteredProjects {
    let result = create_projects_graph(projects, &graph_options(opts));
    FilteredProjects {
        selected_projects: result.graph.keys().cloned().collect(),
        unmatched_filters: Vec::new(),
    }
}
