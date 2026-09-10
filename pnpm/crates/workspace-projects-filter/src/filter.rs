use crate::{
    get_changed_projects::{GetChangedProjectsOptions, get_changed_projects},
    glob,
    parse_project_selector::{ProjectSelector, parse_project_selector},
};
use derive_more::{Display, Error};
use indexmap::IndexSet;
use miette::Diagnostic;
use pnpm_matcher::create_matcher;
use pnpm_workspace_projects_graph::{
    BaseProject, CreateProjectsGraphOptions, GraphProject, ProjectGraph, create_projects_graph,
};
use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
};

/// One raw `--filter` / `--filter-prod` entry, before parsing.
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
    /// Selected project root directories, in selection order.
    pub selected_projects: Vec<PathBuf>,
    pub unmatched_filters: Vec<String>,
}

/// Options for [`filter_workspace_projects`].
#[derive(Debug, Default, Clone)]
pub struct FilterWorkspaceProjectsOptions {
    /// Match directory selectors with glob semantics (`{packages/*}`)
    /// rather than the default subdirectory check.
    pub use_glob_dir_filtering: bool,
    /// Directory a `[<since>]` selector's git diff runs in when the
    /// selector has no `{dir}` part — normally the workspace root.
    pub workspace_dir: PathBuf,
    /// `testPattern`: glob patterns naming test files. A `[<since>]`
    /// selector selects a project whose changed files all match these
    /// patterns without the project's dependents.
    pub test_pattern: Vec<String>,
    /// `changedFilesIgnorePattern`: glob patterns of changed files a
    /// `[<since>]` selector ignores.
    pub changed_files_ignore_pattern: Vec<String>,
}

/// Options for [`filter_projects`] / [`filter_projects_by_selector_objects`].
#[derive(Debug, Clone)]
pub struct FilterProjectsOptions {
    /// Directory that path selectors resolve against.
    pub prefix: PathBuf,
    /// Tri-state `linkWorkspacePackages`, forwarded to
    /// [`create_projects_graph()`].
    pub link_workspace_packages: Option<bool>,
    pub use_glob_dir_filtering: bool,
    /// See [`FilterWorkspaceProjectsOptions::workspace_dir`].
    pub workspace_dir: PathBuf,
    /// See [`FilterWorkspaceProjectsOptions::test_pattern`].
    pub test_pattern: Vec<String>,
    /// See [`FilterWorkspaceProjectsOptions::changed_files_ignore_pattern`].
    pub changed_files_ignore_pattern: Vec<String>,
}

/// Error type of the filter functions.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum FilterError {
    /// The `git diff` behind a `[<since>]` changed-packages selector
    /// failed — unknown revision, not a git repository, or git could
    /// not be spawned. Carries git's stderr under pnpm's
    /// `ERR_PNPM_FILTER_CHANGED` code.
    #[display("Filtering by changed packages failed. {stderr}")]
    #[diagnostic(code(ERR_PNPM_FILTER_CHANGED))]
    FilterChanged {
        #[error(not(source))]
        stderr: String,
    },

    /// A `testPattern` / `changedFilesIgnorePattern` glob did not
    /// compile.
    #[display("Invalid pattern {pattern:?}: {message}")]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_PROJECTS_FILTER_INVALID_PATTERN))]
    InvalidPattern { pattern: String, message: String },

    /// A selector resolved to neither a name pattern, a directory, nor a
    /// diff. The message includes the offending selector so CLI input is
    /// debuggable.
    #[display("Unsupported project selector: {selector}")]
    #[diagnostic(code(ERR_PNPM_WORKSPACE_PROJECTS_FILTER_UNSUPPORTED_SELECTOR))]
    UnsupportedSelector {
        #[error(not(source))]
        selector: String,
    },
}

/// Filter a pre-built [`ProjectGraph`] by `project_selectors`.
///
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
        filter_graph(projects_graph, opts, &include_selectors)?
    };
    let exclude = filter_graph(projects_graph, opts, &exclude_selectors)?;

    let excluded: IndexSet<&PathBuf> = exclude.selected.iter().collect();
    // Keep graph members only: a `[<since>]` selector can surface a
    // changed directory that no workspace project contains (upstream
    // drops those the same way, via its final `pick`).
    let selected_projects: Vec<PathBuf> = include
        .selected
        .into_iter()
        .filter(|dir| !excluded.contains(dir) && projects_graph.contains_key(dir))
        .collect();
    let mut unmatched_filters = include.unmatched_filters;
    unmatched_filters.extend(exclude.unmatched_filters);

    Ok(FilteredProjects { selected_projects, unmatched_filters })
}

struct FilterGraphResult {
    selected: Vec<PathBuf>,
    unmatched_filters: Vec<String>,
}

fn filter_graph<Pkg>(
    projects_graph: &ProjectGraph<Pkg>,
    opts: &FilterWorkspaceProjectsOptions,
    selectors: &[&ProjectSelector],
) -> Result<FilterGraphResult, FilterError>
where
    Pkg: BaseProject,
{
    let mut walk = WalkState::default();
    let mut unmatched_filters: Vec<String> = Vec::new();

    let forward = |id: &Path| Some(projects_graph.get(id)?.dependencies.clone());
    let reversed_graph = selectors
        .iter()
        .any(|selector| selector.include_dependents)
        .then(|| reverse_graph(projects_graph));
    let reverse = |id: &Path| reversed_graph.as_ref()?.get(id).cloned();

    for selector in selectors {
        let mut entry_projects: Option<Vec<PathBuf>>;
        if let Some(diff) = &selector.diff {
            let changed = changed_selector_projects(projects_graph, opts, selector, diff)?;
            entry_projects = Some(changed.changed_projects);
            walk.select_entries(
                WalkFlags { include_dependents: false, ..WalkFlags::of(selector) },
                &changed.ignore_dependent_for_projects,
                &forward,
                &reverse,
            );
        } else {
            entry_projects = match_selector_path(projects_graph, selector, opts);
        }

        if let Some(name_pattern) = &selector.name_pattern {
            entry_projects = Some(match_projects(
                &name_candidates(projects_graph, entry_projects.as_deref()),
                name_pattern,
            ));
        }

        let entry_projects = entry_projects.ok_or_else(|| FilterError::UnsupportedSelector {
            selector: format!("{selector:?}"),
        })?;

        if entry_projects.is_empty() {
            record_unmatched_filter(selector, &mut unmatched_filters);
        }

        walk.select_entries(WalkFlags::of(selector), &entry_projects, &forward, &reverse);
    }

    Ok(FilterGraphResult { selected: walk.into_selected(), unmatched_filters })
}

fn match_selector_path<Pkg: BaseProject>(
    projects_graph: &ProjectGraph<Pkg>,
    selector: &ProjectSelector,
    opts: &FilterWorkspaceProjectsOptions,
) -> Option<Vec<PathBuf>> {
    selector.parent_dir.as_deref().map(|parent_dir| {
        match_projects_by_path(
            projects_graph,
            parent_dir,
            selector.use_glob_dir_filtering.unwrap_or(opts.use_glob_dir_filtering),
        )
    })
}

fn changed_selector_projects<Pkg: BaseProject>(
    projects_graph: &ProjectGraph<Pkg>,
    opts: &FilterWorkspaceProjectsOptions,
    selector: &ProjectSelector,
    diff: &str,
) -> Result<crate::get_changed_projects::ChangedProjects, FilterError> {
    get_changed_projects(
        projects_graph.keys().cloned().collect(),
        diff,
        &GetChangedProjectsOptions {
            workspace_dir: selector.parent_dir.as_deref().unwrap_or(&opts.workspace_dir),
            test_pattern: &opts.test_pattern,
            changed_files_ignore_pattern: &opts.changed_files_ignore_pattern,
        },
    )
}

/// The `(id, manifest name)` pairs a name pattern is matched against: the
/// projects a previous stage of the selector narrowed to, or the whole graph.
fn name_candidates<Pkg>(
    projects_graph: &ProjectGraph<Pkg>,
    entry_projects: Option<&[PathBuf]>,
) -> Vec<(PathBuf, Option<String>)>
where
    Pkg: BaseProject,
{
    let name_of = |id: &Path| {
        projects_graph.get(id).and_then(|node| node.package.manifest_name().map(str::to_string))
    };
    let Some(ids) = entry_projects else {
        return projects_graph
            .iter()
            .map(|(id, node)| (id.clone(), node.package.manifest_name().map(str::to_string)))
            .collect();
    };
    ids.iter().map(|id| (id.clone(), name_of(id))).collect()
}

/// Report the selector that matched nothing, by whichever half named it.
fn record_unmatched_filter(selector: &ProjectSelector, unmatched_filters: &mut Vec<String>) {
    if let Some(name_pattern) = &selector.name_pattern {
        unmatched_filters.push(name_pattern.clone());
    }
    if let Some(parent_dir) = &selector.parent_dir {
        unmatched_filters.push(parent_dir.to_string_lossy().into_owned());
    }
}

/// Select candidate projects whose name matches `pattern`, falling back
/// to a `@*/`-scoped match when an unscoped pattern matches nothing.
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
        let dir_glob = glob::DirGlob::new(&path_starts_with.to_string_lossy());
        projects_graph
            .keys()
            .filter(|id| dir_glob.is_match(&id.to_string_lossy()))
            .cloned()
            .collect()
    } else {
        projects_graph.keys().filter(|id| is_subdir(path_starts_with, id)).cloned().collect()
    }
}

/// Whether `child` is strictly inside `parent`, matching the semantics of
/// the [`is-subdir`](https://github.com/jonschlinkert/is-subdir) package.
fn is_subdir(parent: &Path, child: &Path) -> bool {
    let Some(relative) = pathdiff::diff_paths(child, parent) else {
        return false;
    };
    match relative.components().next() {
        None | Some(Component::ParentDir | Component::CurDir) => false,
        Some(_) => true,
    }
}

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
    let (prod_selectors, all_selectors): (Vec<ProjectSelector>, Vec<ProjectSelector>) =
        selectors.iter().cloned().partition(|selector| selector.follow_prod_deps_only);
    let walk_opts = FilterWorkspaceProjectsOptions {
        use_glob_dir_filtering: opts.use_glob_dir_filtering,
        workspace_dir: opts.workspace_dir.clone(),
        test_pattern: opts.test_pattern.clone(),
        changed_files_ignore_pattern: opts.changed_files_ignore_pattern.clone(),
    };

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

fn select_all_projects<Pkg: GraphProject + Clone>(
    projects: Vec<Pkg>,
    opts: &FilterProjectsOptions,
) -> FilteredProjects {
    let result = create_projects_graph(
        projects,
        &CreateProjectsGraphOptions {
            ignore_dev_deps: false,
            link_workspace_packages: opts.link_workspace_packages,
        },
    );
    FilteredProjects {
        selected_projects: result.graph.keys().cloned().collect(),
        unmatched_filters: Vec::new(),
    }
}

#[cfg(test)]
mod tests;

mod subgraph;
use subgraph::{WalkFlags, WalkState, reverse_graph};
