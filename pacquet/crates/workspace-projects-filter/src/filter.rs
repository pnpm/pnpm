use crate::{
    glob,
    parse_project_selector::{ProjectSelector, parse_project_selector},
};
use derive_more::{Display, Error};
use indexmap::IndexSet;
use miette::Diagnostic;
use pacquet_config::matcher::create_matcher;
use pacquet_workspace_projects_graph::{
    BaseProject, CreateProjectsGraphOptions, GraphProject, ProjectGraph, create_projects_graph,
};
use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
};

/// One raw `--filter` / `--filter-prod` entry, before parsing. Mirrors
/// upstream's
/// [`WorkspaceFilter`](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-filter/src/index.ts#L15-L18).
#[derive(Debug, Clone)]
pub struct WorkspaceFilter {
    pub filter: String,
    /// `true` for `--filter-prod` entries, which follow production
    /// dependencies only.
    pub follow_prod_deps_only: bool,
}

/// Outcome of a filter run: the selected projects (in selection order)
/// and the selectors that matched nothing.
#[derive(Debug, Default, Clone)]
pub struct FilteredProjects {
    /// Selected project root directories, in upstream's
    /// `Object.keys(selectedProjectsGraph)` order.
    pub selected_projects: Vec<PathBuf>,
    pub unmatched_filters: Vec<String>,
}

/// Options for [`filter_workspace_projects`].
#[derive(Debug, Default, Clone, Copy)]
pub struct FilterWorkspaceProjectsOptions {
    /// Match directory selectors with glob semantics (`{packages/*}`)
    /// rather than the default subdirectory check.
    pub use_glob_dir_filtering: bool,
}

/// Options for [`filter_projects`] / [`filter_projects_by_selector_objects`].
#[derive(Debug, Clone)]
pub struct FilterProjectsOptions {
    /// Directory that path selectors resolve against (upstream's
    /// `prefix`).
    pub prefix: PathBuf,
    /// Tri-state `linkWorkspacePackages`, forwarded to
    /// [`create_projects_graph()`].
    pub link_workspace_packages: Option<bool>,
    pub use_glob_dir_filtering: bool,
}

/// Error type of the filter functions.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum FilterError {
    /// A `[<since>]` changed-packages selector was supplied. Resolving
    /// it needs the git-diff project infrastructure pacquet has not
    /// ported yet; the selector parses (so `parse_project_selector`
    /// fills [`ProjectSelector::diff`]) but cannot be evaluated.
    #[display(
        "Changed-package filter selectors (`[<since>]`) are not supported yet: pacquet has not ported the git-diff project selection."
    )]
    #[diagnostic(code(pacquet_workspace_projects_filter::unsupported_diff_selector))]
    UnsupportedDiffSelector,

    /// A selector resolved to neither a name pattern, a directory, nor a
    /// diff. Mirrors upstream's `Unsupported project selector:
    /// ${JSON.stringify(selector)}`, including the offending selector so
    /// CLI input is debuggable.
    #[display("Unsupported project selector: {selector}")]
    #[diagnostic(code(pacquet_workspace_projects_filter::unsupported_selector))]
    UnsupportedSelector {
        #[error(not(source))]
        selector: String,
    },
}

/// Filter a pre-built [`ProjectGraph`] by `project_selectors`.
///
/// Port of upstream's
/// [`filterWorkspaceProjects`](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-filter/src/index.ts#L182-L210).
/// Include selectors are unioned; exclude selectors (`!`-prefixed) are
/// then subtracted. An empty include set means "every project".
pub fn filter_workspace_projects<Pkg>(
    projects_graph: &ProjectGraph<Pkg>,
    project_selectors: &[ProjectSelector],
    opts: &FilterWorkspaceProjectsOptions,
) -> Result<FilteredProjects, FilterError>
where
    Pkg: BaseProject,
{
    let (exclude_selectors, include_selectors): (Vec<&ProjectSelector>, Vec<&ProjectSelector>) =
        project_selectors.iter().partition(|selector| selector.exclude);

    let include = if include_selectors.is_empty() {
        FilterGraphResult {
            selected: projects_graph.keys().cloned().collect(),
            unmatched_filters: Vec::new(),
        }
    } else {
        filter_graph(projects_graph, *opts, &include_selectors)?
    };
    let exclude = filter_graph(projects_graph, *opts, &exclude_selectors)?;

    let excluded: IndexSet<&PathBuf> = exclude.selected.iter().collect();
    let selected_projects: Vec<PathBuf> =
        include.selected.into_iter().filter(|dir| !excluded.contains(dir)).collect();
    let mut unmatched_filters = include.unmatched_filters;
    unmatched_filters.extend(exclude.unmatched_filters);

    Ok(FilteredProjects { selected_projects, unmatched_filters })
}

struct FilterGraphResult {
    selected: Vec<PathBuf>,
    unmatched_filters: Vec<String>,
}

/// Port of upstream's
/// [`_filterGraph`](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-filter/src/index.ts#L212-L298).
fn filter_graph<Pkg>(
    projects_graph: &ProjectGraph<Pkg>,
    opts: FilterWorkspaceProjectsOptions,
    selectors: &[&ProjectSelector],
) -> Result<FilterGraphResult, FilterError>
where
    Pkg: BaseProject,
{
    let mut cherry_picked: Vec<PathBuf> = Vec::new();
    let mut walked_dependencies: IndexSet<PathBuf> = IndexSet::new();
    let mut walked_dependents: IndexSet<PathBuf> = IndexSet::new();
    let mut walked_dependents_dependencies: IndexSet<PathBuf> = IndexSet::new();
    let mut unmatched_filters: Vec<String> = Vec::new();

    let forward = |id: &Path| projects_graph.get(id).map(|node| node.dependencies.clone());
    let reversed_graph = selectors
        .iter()
        .any(|selector| selector.include_dependents)
        .then(|| reverse_graph(projects_graph));
    let reverse = |id: &Path| reversed_graph.as_ref().and_then(|graph| graph.get(id).cloned());

    for selector in selectors {
        if selector.diff.is_some() {
            return Err(FilterError::UnsupportedDiffSelector);
        }

        let mut entry_projects: Option<Vec<PathBuf>> =
            selector.parent_dir.as_deref().map(|parent_dir| {
                match_projects_by_path(projects_graph, parent_dir, opts.use_glob_dir_filtering)
            });

        if let Some(name_pattern) = &selector.name_pattern {
            let candidates: Vec<(PathBuf, Option<String>)> = match &entry_projects {
                None => projects_graph
                    .iter()
                    .map(|(id, node)| {
                        (id.clone(), node.package.manifest_name().map(str::to_string))
                    })
                    .collect(),
                Some(ids) => ids
                    .iter()
                    .map(|id| {
                        let name = projects_graph
                            .get(id)
                            .and_then(|node| node.package.manifest_name().map(str::to_string));
                        (id.clone(), name)
                    })
                    .collect(),
            };
            entry_projects = Some(match_projects(&candidates, name_pattern));
        }

        let Some(entry_projects) = entry_projects else {
            return Err(FilterError::UnsupportedSelector { selector: format!("{selector:?}") });
        };

        if entry_projects.is_empty() {
            if let Some(name_pattern) = &selector.name_pattern {
                unmatched_filters.push(name_pattern.clone());
            }
            if let Some(parent_dir) = &selector.parent_dir {
                unmatched_filters.push(parent_dir.to_string_lossy().into_owned());
            }
        }

        let include_root = !selector.exclude_self;
        if selector.include_dependencies {
            pick_subgraph(&forward, &entry_projects, &mut walked_dependencies, include_root);
        }
        if selector.include_dependents {
            pick_subgraph(&reverse, &entry_projects, &mut walked_dependents, include_root);
        }
        if selector.include_dependencies && selector.include_dependents {
            let dependents: Vec<PathBuf> = walked_dependents.iter().cloned().collect();
            pick_subgraph(&forward, &dependents, &mut walked_dependents_dependencies, false);
        }
        if !selector.include_dependencies && !selector.include_dependents {
            cherry_picked.extend(entry_projects);
        }
    }

    let mut walked: IndexSet<PathBuf> = IndexSet::new();
    walked.extend(walked_dependencies);
    walked.extend(walked_dependents);
    walked.extend(walked_dependents_dependencies);
    for project in cherry_picked {
        walked.insert(project);
    }

    Ok(FilterGraphResult { selected: walked.into_iter().collect(), unmatched_filters })
}

/// Port of upstream's
/// [`pickSubgraph`](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-filter/src/index.ts#L389-L405).
/// `adj` returns the adjacency list (forward dependencies or reversed
/// dependents) for a node; recursion always re-includes the visited
/// children regardless of the top-level `include_root`.
fn pick_subgraph<Adjacency>(
    adj: &Adjacency,
    next_node_ids: &[PathBuf],
    walked: &mut IndexSet<PathBuf>,
    include_root: bool,
) where
    Adjacency: Fn(&Path) -> Option<Vec<PathBuf>>,
{
    for next_node_id in next_node_ids {
        if walked.contains(next_node_id) {
            continue;
        }
        if include_root {
            walked.insert(next_node_id.clone());
        }
        if let Some(children) = adj(next_node_id) {
            pick_subgraph(adj, &children, walked, true);
        }
    }
}

/// Port of upstream's
/// [`reverseGraph`](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-filter/src/index.ts#L307-L320):
/// invert edges so a node maps to the projects that depend on it.
fn reverse_graph<Pkg>(projects_graph: &ProjectGraph<Pkg>) -> HashMap<PathBuf, Vec<PathBuf>> {
    let mut reversed: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for (dependent, node) in projects_graph {
        for dependency in &node.dependencies {
            reversed.entry(dependency.clone()).or_default().push(dependent.clone());
        }
    }
    reversed
}

/// Port of upstream's
/// [`matchProjects`](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-filter/src/index.ts#L322-L334).
/// Falls back to a `@*/<pattern>` scope search when an unscoped pattern
/// matches nothing, but only accepts that fallback when it is
/// unambiguous (exactly one match).
fn match_projects(candidates: &[(PathBuf, Option<String>)], pattern: &str) -> Vec<PathBuf> {
    let matcher = create_matcher(std::slice::from_ref(&pattern.to_string()));
    let matches: Vec<PathBuf> = candidates
        .iter()
        .filter(|(_, name)| name.as_deref().is_some_and(|name| matcher.matches(name)))
        .map(|(id, _)| id.clone())
        .collect();

    if matches.is_empty() && !pattern.starts_with('@') && !pattern.contains('/') {
        let scoped_matches = match_projects(candidates, &format!("@*/{pattern}"));
        return if scoped_matches.len() == 1 { scoped_matches } else { Vec::new() };
    }
    matches
}

fn match_projects_by_path<Pkg>(
    projects_graph: &ProjectGraph<Pkg>,
    path_starts_with: &Path,
    use_glob_dir_filtering: bool,
) -> Vec<PathBuf> {
    if use_glob_dir_filtering {
        let pattern = path_starts_with.to_string_lossy();
        projects_graph
            .keys()
            .filter(|id| glob::is_match(&id.to_string_lossy(), &pattern))
            .cloned()
            .collect()
    } else {
        projects_graph.keys().filter(|id| is_subdir(path_starts_with, id)).cloned().collect()
    }
}

/// Whether `child` is strictly inside `parent`. Mirrors the
/// [`is-subdir`](https://github.com/jonschlinkert/is-subdir) package
/// upstream uses for `matchProjectsByExactPath`: an equal path is *not*
/// a subdirectory.
fn is_subdir(parent: &Path, child: &Path) -> bool {
    let Some(relative) = pathdiff::diff_paths(child, parent) else {
        return false;
    };
    match relative.components().next() {
        // Empty (equal paths), `..`-prefixed (ancestor), or `.` are not
        // subdirectories.
        None | Some(Component::ParentDir | Component::CurDir) => false,
        Some(_) => true,
    }
}

/// Parse and apply a list of [`WorkspaceFilter`]s against `projects`.
///
/// Port of upstream's
/// [`filterProjects`](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-filter/src/index.ts#L92-L101)
/// composed with `filterProjectsBySelectorObjects`.
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

/// Port of upstream's
/// [`filterProjectsBySelectorObjects`](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-filter/src/index.ts#L103-L156).
/// Builds the dependency graph (a `--filter-prod` second graph that
/// ignores devDependencies when any prod selector is present), filters
/// each, and unions the selections (prod first, then all).
pub fn filter_projects_by_selector_objects<Pkg>(
    projects: Vec<Pkg>,
    selectors: &[ProjectSelector],
    opts: &FilterProjectsOptions,
) -> Result<FilteredProjects, FilterError>
where
    Pkg: GraphProject + Clone,
{
    let (prod_selectors, all_selectors): (Vec<ProjectSelector>, Vec<ProjectSelector>) =
        selectors.iter().cloned().partition(|selector| selector.follow_prod_deps_only);
    let walk_opts =
        FilterWorkspaceProjectsOptions { use_glob_dir_filtering: opts.use_glob_dir_filtering };

    if all_selectors.is_empty() && prod_selectors.is_empty() {
        let result = create_projects_graph(
            projects,
            &CreateProjectsGraphOptions {
                ignore_dev_deps: false,
                link_workspace_packages: opts.link_workspace_packages,
            },
        );
        return Ok(FilteredProjects {
            selected_projects: result.graph.keys().cloned().collect(),
            unmatched_filters: Vec::new(),
        });
    }

    let mut selected: IndexSet<PathBuf> = IndexSet::new();
    let mut unmatched_filters: Vec<String> = Vec::new();

    if !prod_selectors.is_empty() {
        let prod_graph = create_projects_graph(
            projects.clone(),
            &CreateProjectsGraphOptions {
                ignore_dev_deps: true,
                link_workspace_packages: opts.link_workspace_packages,
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
                ignore_dev_deps: false,
                link_workspace_packages: opts.link_workspace_packages,
            },
        )
        .graph;
        let result = filter_workspace_projects(&graph, &all_selectors, &walk_opts)?;
        selected.extend(result.selected_projects);
        unmatched_filters.extend(result.unmatched_filters);
    }

    Ok(FilteredProjects { selected_projects: selected.into_iter().collect(), unmatched_filters })
}

#[cfg(test)]
mod tests;
