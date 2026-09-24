pub mod from_projects;
pub use from_projects::{filter_projects, filter_projects_by_selector_objects};

use crate::{
    get_changed_projects::{GetChangedProjectsOptions, get_changed_projects},
    glob,
    parse_project_selector::{DependencyTraversal, ProjectSelector},
};
use derive_more::{Display, Error};
use indexmap::IndexSet;
use miette::Diagnostic;
use pnpm_catalogs_types::Catalogs;
use pnpm_matcher::create_matcher;
use pnpm_workspace_projects_graph::{BaseProject, ProjectGraph};
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
    /// [`create_projects_graph()`](pnpm_workspace_projects_graph::create_projects_graph).
    pub link_workspace_packages: Option<bool>,
    pub use_glob_dir_filtering: bool,
    /// See [`FilterWorkspaceProjectsOptions::workspace_dir`].
    pub workspace_dir: PathBuf,
    /// See [`FilterWorkspaceProjectsOptions::test_pattern`].
    pub test_pattern: Vec<String>,
    /// See [`FilterWorkspaceProjectsOptions::changed_files_ignore_pattern`].
    pub changed_files_ignore_pattern: Vec<String>,
    /// The workspace catalogs a `catalog:` dependency resolves through,
    /// with relative paths measured from `workspace_dir`.
    pub catalogs: Option<Catalogs>,
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

struct SelectorChunk<'a> {
    exclude: bool,
    selectors: Vec<&'a ProjectSelector>,
}

fn chunk_selectors<'a>(project_selectors: &'a [ProjectSelector]) -> Vec<SelectorChunk<'a>> {
    let mut chunks: Vec<SelectorChunk<'a>> = Vec::new();
    for selector in project_selectors {
        if let Some(last) = chunks
            .last_mut()
            .filter(|last| last.exclude == selector.exclude)
        {
            last.selectors.push(selector);
            continue;
        }
        chunks.push(SelectorChunk { exclude: selector.exclude, selectors: vec![selector] });
    }
    chunks
}

fn apply_chunk(selected: &mut IndexSet<PathBuf>, chunk_selected: Vec<PathBuf>, exclude: bool) {
    if exclude {
        for dir in &chunk_selected {
            selected.shift_remove(dir);
        }
    } else {
        for dir in chunk_selected {
            selected.insert(dir);
        }
    }
}

/// Filter a pre-built [`ProjectGraph`] by `project_selectors`.
///
/// Selectors are evaluated in ordered chunks of matching polarity. When the
/// first selector is an exclusion, selection begins with every project in the
/// workspace; otherwise it starts empty. Subsequent inclusion selectors re-include
/// projects that were previously excluded.
pub fn filter_workspace_projects<Pkg>(
    projects_graph: &ProjectGraph<Pkg>,
    project_selectors: &[ProjectSelector],
    opts: &FilterWorkspaceProjectsOptions,
) -> Result<FilteredProjects, FilterError>
where
    Pkg: BaseProject,
{
    if project_selectors.is_empty() {
        return Ok(FilteredProjects {
            selected_projects: projects_graph.keys().cloned().collect(),
            unmatched_filters: Vec::new(),
        });
    }

    let chunks = chunk_selectors(project_selectors);
    let mut selected: IndexSet<PathBuf> = if chunks.first().is_some_and(|c| c.exclude) {
        projects_graph.keys().cloned().collect()
    } else {
        IndexSet::new()
    };
    let mut unmatched_filters = Vec::new();

    for chunk in chunks {
        let result = filter_graph(projects_graph, opts, &chunk.selectors)?;
        unmatched_filters.extend(result.unmatched_filters);
        apply_chunk(&mut selected, result.selected, chunk.exclude);
    }

    // Keep graph members only: a `[<since>]` selector can surface a
    // changed directory that no workspace project contains (upstream
    // drops those the same way, via its final `pick`).
    let selected_projects: Vec<PathBuf> = selected
        .into_iter()
        .filter(|dir| projects_graph.contains_key(dir))
        .collect();

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
        .any(|selector| selector.traversal.include_dependents)
        .then(|| reverse_graph(projects_graph));
    let reverse = |id: &Path| {
        reversed_graph
            .as_ref()
            .and_then(|graph| graph.get(id).cloned())
    };

    for selector in selectors {
        let entry_projects = resolve_selector_entries(
            selector,
            projects_graph,
            opts,
            &mut walk,
            &forward,
            &reverse,
        )?;

        if entry_projects.is_empty() {
            record_unmatched_filter(selector, &mut unmatched_filters);
        }

        walk.select_entries(selector.traversal, &entry_projects, &forward, &reverse);
    }

    Ok(FilterGraphResult { selected: walk.into_selected(), unmatched_filters })
}

fn resolve_selector_entries<Pkg, FForward, FReverse>(
    selector: &ProjectSelector,
    projects_graph: &ProjectGraph<Pkg>,
    opts: &FilterWorkspaceProjectsOptions,
    walk: &mut WalkState,
    forward: &FForward,
    reverse: &FReverse,
) -> Result<Vec<PathBuf>, FilterError>
where
    Pkg: BaseProject,
    FForward: Fn(&Path) -> Option<Vec<PathBuf>>,
    FReverse: Fn(&Path) -> Option<Vec<PathBuf>>,
{
    let mut entry_projects: Option<Vec<PathBuf>>;
    if let Some(diff) = &selector.diff {
        let changed = changed_selector_projects(projects_graph, opts, selector, diff)?;
        entry_projects = Some(changed.changed_projects);
        walk.select_entries(
            DependencyTraversal { include_dependents: false, ..selector.traversal },
            &changed.ignore_dependent_for_projects,
            forward,
            reverse,
        );
    } else {
        entry_projects = match_selector_path(projects_graph, selector, opts);
    }

    if let Some(name_pattern) = &selector.name_pattern {
        entry_projects =
            Some(match_named_entries(projects_graph, entry_projects.as_deref(), name_pattern));
    }

    entry_projects.ok_or_else(|| FilterError::UnsupportedSelector {
        selector: format!("{selector:?}"),
    })
}

fn match_selector_path<Pkg: BaseProject>(
    projects_graph: &ProjectGraph<Pkg>,
    selector: &ProjectSelector,
    opts: &FilterWorkspaceProjectsOptions,
) -> Option<Vec<PathBuf>> {
    selector.parent_dir
        .as_deref()
        .map(|parent_dir| {
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
    let project_dependencies: HashMap<PathBuf, Vec<(String, String)>> = projects_graph
        .iter()
        .map(|(dir, node)| (dir.clone(), node.package.merged_dependencies(false)))
        .collect();
    get_changed_projects(
        projects_graph.keys().cloned().collect(),
        diff,
        &GetChangedProjectsOptions {
            workspace_dir: &opts.workspace_dir,
            working_dir: selector.parent_dir.as_deref(),
            test_pattern: &opts.test_pattern,
            changed_files_ignore_pattern: &opts.changed_files_ignore_pattern,
            project_dependencies: Some(&project_dependencies),
            use_glob_dir_filtering: selector.use_glob_dir_filtering.unwrap_or(
                opts.use_glob_dir_filtering,
            ),
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
        projects_graph
            .get(id)
            .and_then(|node| node.package.manifest_name().map(str::to_string))
    };
    let Some(ids) = entry_projects else {
        return projects_graph
            .iter()
            .map(|(id, node)| (id.clone(), node.package.manifest_name().map(str::to_string)))
            .collect();
    };
    ids.iter()
        .map(|id| (id.clone(), name_of(id)))
        .collect()
}

/// Report the selector that matched nothing, by whichever half named it.
fn match_named_entries<Pkg: BaseProject>(
    graph: &ProjectGraph<Pkg>,
    entries: Option<&[PathBuf]>,
    pattern: &str,
) -> Vec<PathBuf> {
    match_projects(&name_candidates(graph, entries), pattern)
}

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
        .filter(|(_, name)| {
            name.as_deref()
                .is_some_and(|name| matcher.matches(name))
        })
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
        projects_graph
            .keys()
            .filter(|id| is_subdir(path_starts_with, id))
            .cloned()
            .collect()
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

#[cfg(test)]
mod tests;

mod subgraph;
use subgraph::{WalkState, reverse_graph};
