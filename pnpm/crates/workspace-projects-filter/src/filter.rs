use crate::{
    get_changed_projects::{GetChangedProjectsOptions, get_changed_projects},
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
    #[diagnostic(code(pacquet_workspace_projects_filter::invalid_pattern))]
    InvalidPattern { pattern: String, message: String },

    /// A selector resolved to neither a name pattern, a directory, nor a
    /// diff. The message includes the offending selector so CLI input is
    /// debuggable.
    #[display("Unsupported project selector: {selector}")]
    #[diagnostic(code(pacquet_workspace_projects_filter::unsupported_selector))]
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

    let forward = |id: &Path| projects_graph.get(id).map(|node| node.dependencies.clone());
    let reversed_graph = selectors
        .iter()
        .any(|selector| selector.include_dependents)
        .then(|| reverse_graph(projects_graph));
    let reverse = |id: &Path| reversed_graph.as_ref().and_then(|graph| graph.get(id).cloned());

    for selector in selectors {
        let mut entry_projects: Option<Vec<PathBuf>> = None;
        if let Some(diff) = &selector.diff {
            let changed = get_changed_projects(
                projects_graph.keys().cloned().collect(),
                diff,
                &GetChangedProjectsOptions {
                    workspace_dir: selector.parent_dir.as_deref().unwrap_or(&opts.workspace_dir),
                    test_pattern: &opts.test_pattern,
                    changed_files_ignore_pattern: &opts.changed_files_ignore_pattern,
                },
            )?;
            entry_projects = Some(changed.changed_projects);
            walk.select_entries(
                WalkFlags { include_dependents: false, ..WalkFlags::of(selector) },
                &changed.ignore_dependent_for_projects,
                &forward,
                &reverse,
            );
        } else if let Some(parent_dir) = selector.parent_dir.as_deref() {
            entry_projects = Some(match_projects_by_path(
                projects_graph,
                parent_dir,
                opts.use_glob_dir_filtering,
            ));
        }

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

        walk.select_entries(WalkFlags::of(selector), &entry_projects, &forward, &reverse);
    }

    Ok(FilterGraphResult { selected: walk.into_selected(), unmatched_filters })
}

/// The selector modifiers that drive [`WalkState::select_entries`]. A
/// `[<since>]` selector selects its test-only-changed projects through
/// a copy with `include_dependents` suppressed.
#[derive(Clone, Copy)]
struct WalkFlags {
    include_dependencies: bool,
    include_dependents: bool,
    exclude_self: bool,
}

impl WalkFlags {
    fn of(selector: &ProjectSelector) -> Self {
        WalkFlags {
            include_dependencies: selector.include_dependencies,
            include_dependents: selector.include_dependents,
            exclude_self: selector.exclude_self,
        }
    }
}

/// Accumulates the projects the selectors of one [`filter_graph`] run
/// pick, in the buckets whose union (dependencies, dependents,
/// dependents' dependencies, then cherry-picks) fixes the selection
/// order.
#[derive(Default)]
struct WalkState {
    cherry_picked: Vec<PathBuf>,
    walked_dependencies: IndexSet<PathBuf>,
    walked_dependents: IndexSet<PathBuf>,
    walked_dependents_dependencies: IndexSet<PathBuf>,
}

impl WalkState {
    fn select_entries<Forward, Reverse>(
        &mut self,
        flags: WalkFlags,
        entry_projects: &[PathBuf],
        forward: &Forward,
        reverse: &Reverse,
    ) where
        Forward: Fn(&Path) -> Option<Vec<PathBuf>>,
        Reverse: Fn(&Path) -> Option<Vec<PathBuf>>,
    {
        let include_root = !flags.exclude_self;
        if flags.include_dependencies {
            pick_subgraph(forward, entry_projects, &mut self.walked_dependencies, include_root);
        }
        if flags.include_dependents {
            pick_subgraph(reverse, entry_projects, &mut self.walked_dependents, include_root);
        }
        if flags.include_dependencies && flags.include_dependents {
            let dependents: Vec<PathBuf> = self.walked_dependents.iter().cloned().collect();
            pick_subgraph(forward, &dependents, &mut self.walked_dependents_dependencies, false);
        }
        if !flags.include_dependencies && !flags.include_dependents {
            self.cherry_picked.extend(entry_projects.iter().cloned());
        }
    }

    fn into_selected(self) -> Vec<PathBuf> {
        let mut walked: IndexSet<PathBuf> = IndexSet::new();
        walked.extend(self.walked_dependencies);
        walked.extend(self.walked_dependents);
        walked.extend(self.walked_dependents_dependencies);
        walked.extend(self.cherry_picked);
        walked.into_iter().collect()
    }
}

/// Walk the subgraph reachable from `next_node_ids`.
///
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

/// Invert edges so a node maps to the projects that depend on it.
fn reverse_graph<Pkg>(projects_graph: &ProjectGraph<Pkg>) -> HashMap<PathBuf, Vec<PathBuf>> {
    let mut reversed: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for (dependent, node) in projects_graph {
        for dependency in &node.dependencies {
            reversed.entry(dependency.clone()).or_default().push(dependent.clone());
        }
    }
    reversed
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
