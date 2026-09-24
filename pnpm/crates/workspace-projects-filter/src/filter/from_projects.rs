use super::{
    FilterError, FilterProjectsOptions, FilterWorkspaceProjectsOptions, FilteredProjects,
    WorkspaceFilter, filter_workspace_projects,
};
use crate::parse_project_selector::{ProjectSelector, parse_project_selector};
use indexmap::IndexSet;
use pnpm_workspace_projects_graph::{
    CreateProjectsGraphOptions, GraphProject, create_projects_graph,
};
use std::path::PathBuf;

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
/// against the full graph.
pub fn filter_projects_by_selector_objects<Pkg>(
    projects: Vec<Pkg>,
    selectors: &[ProjectSelector],
    opts: &FilterProjectsOptions,
) -> Result<FilteredProjects, FilterError>
where
    Pkg: GraphProject + Clone,
{
    let (prod_selectors, all_selectors): (Vec<ProjectSelector>, Vec<ProjectSelector>) = selectors
        .iter()
        .cloned()
        .partition(|selector| selector.follow_prod_deps_only);
    let walk_opts = workspace_filter_options(opts);

    if all_selectors.is_empty() && prod_selectors.is_empty() {
        return Ok(select_all_projects(projects, opts));
    }

    let mut selected: IndexSet<PathBuf> = IndexSet::new();
    let mut unmatched_filters: Vec<String> = Vec::new();

    if !prod_selectors.is_empty() {
        let prod_graph = create_projects_graph(
            projects.clone(),
            &CreateProjectsGraphOptions {
                ignore_dev_deps: true,
                link_workspace_packages: opts.link_workspace_packages,
                ..CreateProjectsGraphOptions::default()
            },
        )
        .graph;
        let result = filter_workspace_projects(&prod_graph, &prod_selectors, &walk_opts)?;
        selected.extend(result.selected_projects);
        unmatched_filters.extend(result.unmatched_filters);
    }

    if !all_selectors.is_empty() {
        let graph = create_projects_graph(
            projects,
            &CreateProjectsGraphOptions {
                link_workspace_packages: opts.link_workspace_packages,
                ..CreateProjectsGraphOptions::default()
            },
        )
        .graph;
        let result = filter_workspace_projects(&graph, &all_selectors, &walk_opts)?;
        selected.extend(result.selected_projects);
        unmatched_filters.extend(result.unmatched_filters);
    }

    Ok(FilteredProjects { selected_projects: selected.into_iter().collect(), unmatched_filters })
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
    let result = create_projects_graph(
        projects,
        &CreateProjectsGraphOptions {
            link_workspace_packages: opts.link_workspace_packages,
            ..CreateProjectsGraphOptions::default()
        },
    );
    FilteredProjects {
        selected_projects: result.graph.keys().cloned().collect(),
        unmatched_filters: Vec::new(),
    }
}
