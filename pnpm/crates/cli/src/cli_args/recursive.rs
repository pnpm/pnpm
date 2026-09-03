//! Shared machinery for the recursive (`-r`) variants of `run` and
//! `exec`: workspace-project discovery, `--filter` selection,
//! dependency graph construction, `--resume-from` trimming, and the
//! `pnpm-exec-summary.json` execution-status report.
//!
//! The per-command pieces (which action runs per project, and the
//! command-specific error codes) live in `run/recursive.rs` and
//! `exec/recursive.rs`.

use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_config::{Config, LinkWorkspacePackages};
use pnpm_package_manager::{GraphSequencerResult, graph_sequencer};
use pnpm_workspace::{
    FindWorkspaceProjectsOpts, GraphPkg, Project, find_workspace_projects,
    importer_id_from_root_dir, read_workspace_manifest, workspace_package_patterns,
};
use pnpm_workspace_projects_filter::{
    FilterWorkspaceProjectsOptions, ProjectSelector, filter_workspace_projects,
    parse_project_selector,
};
use pnpm_workspace_projects_graph::{
    BaseProject, CreateProjectsGraphOptions, ProjectGraph, create_projects_graph,
};
use rayon::prelude::*;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

/// `Cannot find package {resume_from}` — raised by both recursive `run`
/// and recursive `exec` when `--resume-from` names a package that is not
/// in the workspace. Shares pnpm's `RESUME_FROM_NOT_FOUND` code across
/// both commands.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("Cannot find package {resume_from}. Could not determine where to resume from.")]
#[diagnostic(code(ERR_PNPM_RESUME_FROM_NOT_FOUND))]
pub struct ResumeFromNotFound {
    #[error(not(source))]
    pub resume_from: String,
}

/// Diagnostic code of [`NoMatchingProjects`]. pnpm has no error code for
/// an empty selection — it prints the sentence and sets the exit code — so
/// this one exists only for `is_reported_error` to recognize the failure as
/// already printed, and is never rendered.
pub const NO_MATCHING_PROJECTS_CODE: &str = "ERR_PNPM_NO_MATCHING_PROJECTS";

/// `--fail-if-no-match` with an empty workspace-project selection. The
/// message is already on stdout by the time this is returned; see
/// [`ensure_projects_matched`].
#[derive(Debug, Display, Error, Diagnostic)]
#[display("{message}")]
#[diagnostic(code(ERR_PNPM_NO_MATCHING_PROJECTS))]
pub struct NoMatchingProjects {
    #[error(not(source))]
    pub message: String,
}

/// The dependency edges among the `--filter`-selected projects, resolved
/// through the full workspace graph so a relationship between two selected
/// projects via an unselected one becomes a direct edge. Keys keep the
/// selection order.
pub fn filtered_projects_dependencies<Pkg: Sync>(
    selected: &ProjectGraph<Pkg>,
    all: &ProjectGraph<Pkg>,
    prod_all: Option<&ProjectGraph<Pkg>>,
    prod_only_selected: &HashSet<PathBuf>,
) -> IndexMap<PathBuf, Vec<PathBuf>> {
    let sorted: HashSet<&Path> = selected.keys().map(PathBuf::as_path).collect();
    // Each project's tunneling walk reads only shared references, so
    // the projects fan out across the rayon pool; collecting the
    // parallel iterator into a `Vec` keeps the selection order.
    selected
        .keys()
        .collect::<Vec<_>>()
        .par_iter()
        .map(|&project_dir| {
            let full_graph = match prod_all {
                Some(prod_all) if prod_only_selected.contains(project_dir) => prod_all,
                _ => all,
            };
            (project_dir.clone(), sorted_dependencies(selected, full_graph, project_dir, &sorted))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .collect()
}

/// Sequence `projects_graph` into one deterministic topological order,
/// resolving transitive edges through `full_projects_graph`.
pub fn sequence_graph<Pkg>(
    projects_graph: &ProjectGraph<Pkg>,
    full_projects_graph: &ProjectGraph<Pkg>,
) -> GraphSequencerResult<PathBuf> {
    sequence_graph_by_project(projects_graph, |_| full_projects_graph)
}

/// Sequence `projects_graph`, resolving each project's transitive edges
/// through the full graph that `full_graph_for` returns for it. A
/// `--filter-prod` selection routes its projects to the prod-pruned graph so
/// pruned dev edges stay pruned, while regular projects route to the full
/// graph.
fn sequence_graph_by_project<'g, Pkg: 'g>(
    projects_graph: &ProjectGraph<Pkg>,
    full_graph_for: impl Fn(&Path) -> &'g ProjectGraph<Pkg>,
) -> GraphSequencerResult<PathBuf> {
    let sorted_dirs: Vec<PathBuf> = projects_graph.keys().cloned().collect();
    let sorted: HashSet<&Path> = sorted_dirs.iter().map(PathBuf::as_path).collect();
    let dependency_graph: HashMap<PathBuf, Vec<PathBuf>> = projects_graph
        .keys()
        .map(|project_dir| {
            let dependencies = sorted_dependencies(
                projects_graph,
                full_graph_for(project_dir),
                project_dir,
                &sorted,
            );
            (project_dir.clone(), dependencies)
        })
        .collect();
    graph_sequencer(&dependency_graph, &sorted_dirs)
}

/// The dependencies of `project_dir` that are themselves in `sorted`, reached
/// by tunneling past any project outside `sorted`. A transitive dependency
/// between two sorted projects thus becomes a direct edge.
///
/// `project_dir`'s own edges are read from `projects_graph`, so a selection
/// that deliberately narrows them (e.g. a prod-only filter that drops dev
/// edges) is respected; `full_projects_graph` is consulted only to walk
/// through the projects outside `sorted`.
fn sorted_dependencies<Pkg>(
    projects_graph: &ProjectGraph<Pkg>,
    full_projects_graph: &ProjectGraph<Pkg>,
    project_dir: &Path,
    sorted: &HashSet<&Path>,
) -> Vec<PathBuf> {
    let mut dependencies: Vec<PathBuf> = Vec::new();
    // Borrowed paths and an FxHash set: this walk runs once per
    // selected project, and cloning every visited `PathBuf` into a
    // SipHash set dominated it on a workspace-scale graph.
    let mut visited: rustc_hash::FxHashSet<&Path> = rustc_hash::FxHashSet::default();
    let mut stack: Vec<&Path> = projects_graph
        .get(project_dir)
        .map(|node| node.dependencies.iter().map(PathBuf::as_path).collect())
        .unwrap_or_default();
    while let Some(dependency_dir) = stack.pop() {
        if dependency_dir == project_dir || !visited.insert(dependency_dir) {
            continue;
        }
        if sorted.contains(dependency_dir) {
            dependencies.push(dependency_dir.to_path_buf());
        } else if let Some(node) = full_projects_graph.get(dependency_dir) {
            stack.extend(node.dependencies.iter().map(PathBuf::as_path));
        }
    }
    dependencies
}

/// The project directory `--resume-from` names, located by manifest name;
/// an unknown name is a [`ResumeFromNotFound`] error. The invocation's
/// task for that project anchors the resumed task graph.
pub fn find_resume_root(
    resume_from: &str,
    graph: &ProjectGraph<GraphPkg<'_>>,
) -> Result<PathBuf, ResumeFromNotFound> {
    graph
        .iter()
        .find(|(_, node)| node.package.manifest_name() == Some(resume_from))
        .map(|(root, _)| root.clone())
        .ok_or_else(|| ResumeFromNotFound { resume_from: resume_from.to_string() })
}

/// Write the recursive summary to `pnpm-exec-summary.json` under `dir`.
///
/// The per-task map is nested under an `executionStatus` key. Keys are
/// project directories, `#`-qualified with the task name for tasks
/// `dependsOn` pulled in — see `task_summary_key`.
pub fn write_recursive_summary(
    dir: &Path,
    summary: &IndexMap<String, ExecutionStatus>,
) -> miette::Result<()> {
    let path = dir.join("pnpm-exec-summary.json");
    let mut contents =
        serde_json::to_string_pretty(&ExecSummaryFile { execution_status: summary.clone() })
            .into_diagnostic()?;
    contents.push('\n');
    std::fs::write(&path, contents)
        .into_diagnostic()
        .wrap_err_with(|| format!("writing {}", path.display()))
}

/// Count the tasks whose action failed.
///
/// The caller turns a non-zero count into its command-specific
/// `ERR_PNPM_RECURSIVE_FAIL` error. Skipped dependents of a failed task do
/// not add to the count: the failure that blocked them is already counted.
pub fn count_failures(summary: &IndexMap<String, ExecutionStatus>) -> usize {
    summary.values().filter(|status| status.status == Status::Failure).count()
}

/// Enumerate the projects of the workspace rooted at `workspace_root`,
/// returning them alongside the package patterns that selected them.
/// Shared by recursive `run` / `exec` / `pack` so all discover the same
/// set before [`select_recursive_projects`] narrows it. The patterns feed
/// the root-only guard of [`AutoExcludeRoot`]; `None` means no
/// `pnpm-workspace.yaml` was found.
///
/// The patterns come from `config.workspace_package_patterns`, already
/// resolved from `--workspace-packages` or the manifest's `packages`.
/// When the config found no workspace, `workspace_root` is one the caller
/// picked itself (a global install's packages dir), so its own manifest
/// decides — unless `--ignore-workspace` disowned every manifest, which
/// leaves the enumeration on its `['.', '**']` default.
pub fn discover_workspace_projects(
    workspace_root: &Path,
    config: &Config,
) -> miette::Result<(Vec<Project>, Option<Vec<String>>)> {
    let patterns = match &config.workspace_package_patterns {
        Some(patterns) => Some(patterns.clone()),
        None if config.ignore_workspace => None,
        None => read_workspace_manifest(workspace_root)
            .into_diagnostic()
            .wrap_err("reading pnpm-workspace.yaml")?
            .map(|manifest| workspace_package_patterns(&manifest)),
    };
    let projects = find_workspace_projects(
        workspace_root,
        &FindWorkspaceProjectsOpts { patterns: patterns.clone() },
    )
    .wrap_err("finding workspace projects")?;
    Ok((projects, patterns))
}

/// The `--filter`-selected workspace projects plus the graphs the sort
/// resolves order through. `selected` is what the recursive command runs.
/// `all` is the full workspace graph, used to resolve edges that pass
/// through unselected projects; it is `None` for an unfiltered run, where
/// `selected` already is the full graph, so it need not be duplicated.
/// `prod_all` is the prod-pruned full graph, present only when a
/// `--filter-prod` selector is active, and `prod_only_selected` names the
/// projects selected solely by `--filter-prod` so the sort routes them
/// through `prod_all`.
pub struct RecursiveSelection<'a> {
    pub selected: ProjectGraph<GraphPkg<'a>>,
    pub all: Option<ProjectGraph<GraphPkg<'a>>>,
    pub prod_all: Option<ProjectGraph<GraphPkg<'a>>>,
    pub prod_only_selected: HashSet<PathBuf>,
}

impl<'a> RecursiveSelection<'a> {
    /// The full graph the sort resolves transitive edges through: `all` when
    /// present, otherwise `selected`. See the `all` field for why `selected`
    /// suffices when nothing narrowed the run.
    pub fn full_graph(&self) -> &ProjectGraph<GraphPkg<'a>> {
        self.all.as_ref().unwrap_or(&self.selected)
    }
}

/// Build the `--filter`-selected workspace projects the recursive command
/// runs over, together with the graphs [`filtered_projects_dependencies`] resolves
/// order through. `prefix` is where path selectors resolve; `auto_exclude_root`
/// applies the main-dispatch `!{<workspace-root>}` augmentation for
/// `run` / `exec`.
///
/// An unnarrowed run — no `--filter` / `--filter-prod` selector and no root
/// auto-exclusion — returns every project and leaves `all` unset; any
/// narrowing populates `all` (and `prod_all` for `--filter-prod`) for the
/// sort to resolve order through.
pub fn select_recursive_projects<'a>(
    projects: &'a [Project],
    config: &Config,
    prefix: &Path,
    auto_exclude_root: AutoExcludeRoot<'_>,
) -> miette::Result<RecursiveSelection<'a>> {
    // The filter graphs are built with the configured `link-workspace-packages`
    // policy. Under the default `link-workspace-packages: false` a bare-semver
    // range naming a sibling is not a workspace edge, so it drives neither
    // selection nor order; only a `workspace:` range or an enabled policy links
    // it.
    let graph_options = CreateProjectsGraphOptions {
        link_workspace_packages: Some(config.link_workspace_packages != LinkWorkspacePackages::Off),
        ..CreateProjectsGraphOptions::default()
    };
    let all = build_graph(projects, graph_options);

    // Routes into the selection pass whose `follow_prod_deps_only` matches: the
    // prod pass when a `--filter-prod` selector is present, otherwise the
    // regular pass.
    let root_selector = auto_exclude_root.root_selector(config, prefix);

    if config.filter.is_empty() && config.filter_prod.is_empty() && root_selector.is_none() {
        ensure_projects_matched(all.len(), all.len(), config, prefix)?;
        return Ok(RecursiveSelection {
            selected: all,
            all: None,
            prod_all: None,
            prod_only_selected: HashSet::new(),
        });
    }

    // Run the filters against the graphs already built here, so nothing is
    // rebuilt inside the filter call and each selected set is drawn from the
    // very graph the sort resolves order through. The regular and prod-only
    // selectors run separately so the projects a `--filter-prod` selector
    // contributes can be sorted through the prod-pruned graph; their union is
    // the same set a single combined filter call would return.
    let prod_all = if config.filter_prod.is_empty() {
        None
    } else {
        Some(build_graph(
            projects,
            CreateProjectsGraphOptions { ignore_dev_deps: true, ..graph_options },
        ))
    };

    let root_in_prod = !config.filter_prod.is_empty();
    let walk_opts = FilterWorkspaceProjectsOptions {
        // The mode user-written `{<dir>}` selectors match in. The
        // generated `!{<workspace-root>}` selector pins itself to glob
        // matching instead — see `filter_against`.
        use_glob_dir_filtering: !config.legacy_dir_filtering,
        workspace_dir: config.workspace_dir.as_deref().unwrap_or(prefix).to_path_buf(),
        test_pattern: config.test_pattern.clone(),
        changed_files_ignore_pattern: config.changed_files_ignore_pattern.clone(),
    };
    let regular_selected = filter_against(
        &all,
        &config.filter,
        root_selector.as_deref().filter(|_| !root_in_prod),
        false,
        prefix,
        &walk_opts,
    )?;
    let prod_selected = match &prod_all {
        Some(prod_all) => filter_against(
            prod_all,
            &config.filter_prod,
            root_selector.as_deref().filter(|_| root_in_prod),
            true,
            prefix,
            &walk_opts,
        )?,
        None => Vec::new(),
    };

    let mut selected: ProjectGraph<GraphPkg<'a>> = ProjectGraph::new();
    let mut prod_only_selected: HashSet<PathBuf> = HashSet::new();

    // Order and node assignment: prod-selected projects come first with their
    // prod-pruned edges, so the sort never sees the dev edges that selection
    // dropped. A project also matched by a regular selector keeps this earlier
    // position but has its node overwritten with the full-graph one below, and
    // is left out of `prod_only_selected`. Insertion order is user-visible: the
    // recursive runners use it as the dispatch tie-break order.
    if let Some(prod_all) = &prod_all {
        let regular: HashSet<&PathBuf> = regular_selected.iter().collect();
        for dir in &prod_selected {
            if let Some(node) = prod_all.get(dir) {
                selected.insert(dir.clone(), node.clone());
                if !regular.contains(dir) {
                    prod_only_selected.insert(dir.clone());
                }
            }
        }
    }
    // Regular-selected projects keep their full (dev-inclusive) edges,
    // overwriting the prod node for any project selected both ways.
    for dir in &regular_selected {
        if let Some(node) = all.get(dir) {
            selected.insert(dir.clone(), node.clone());
        }
    }

    ensure_projects_matched(selected.len(), all.len(), config, prefix)?;
    Ok(RecursiveSelection { selected, all: Some(all), prod_all, prod_only_selected })
}

/// pnpm's `--fail-if-no-match`: a selection that came back empty ends the
/// run with exit code 1 instead of letting the command operate on no
/// project at all.
///
/// pnpm prints the sentence to stdout and sets `process.exitCode = 1`, so
/// the message is printed here and the returned error carries
/// [`NO_MATCHING_PROJECTS_CODE`], which `is_reported_error` recognizes as
/// already-printed.
fn ensure_projects_matched(
    selected_count: usize,
    all_count: usize,
    config: &Config,
    prefix: &Path,
) -> miette::Result<()> {
    if !config.fail_if_no_match || selected_count != 0 {
        return Ok(());
    }
    let workspace_dir = notice_workspace_dir(config, prefix);
    let message = if all_count == 0 {
        format!(r#"No projects found in "{}""#, workspace_dir.display())
    } else {
        no_projects_matched_message(workspace_dir)
    };
    println!("{message}");
    Err(NoMatchingProjects { message }.into())
}

/// The directory pnpm names in its empty-selection notices: the workspace
/// root when the run found one, else where the command was invoked.
pub fn notice_workspace_dir<'a>(config: &'a Config, prefix: &'a Path) -> &'a Path {
    config.workspace_dir.as_deref().unwrap_or(prefix)
}

/// pnpm's notice for `--filter` / `--filter-prod` selectors that selected
/// no workspace project. pnpm prints it and skips the command; a command
/// that would otherwise emit output for the empty selection prints this
/// first.
pub fn no_projects_matched_message(workspace_dir: &Path) -> String {
    format!(r#"No projects matched the filters in "{}""#, workspace_dir.display())
}

/// The lockfile importer ids of `selection`'s projects, in selection
/// order. The ids key the lockfile's `importers` map, so a lockfile-driven
/// command (`licenses`, `sbom`) can narrow its walk to what `--filter` /
/// `--filter-prod` selected.
pub fn selected_importer_ids(
    selection: &RecursiveSelection<'_>,
    lockfile_dir: &Path,
) -> Vec<String> {
    selection
        .selected
        .keys()
        .map(|project_dir| importer_id_from_root_dir(lockfile_dir, project_dir))
        .collect()
}

/// Build the workspace [`ProjectGraph`] from `projects` under `options`.
fn build_graph(
    projects: &[Project],
    options: CreateProjectsGraphOptions,
) -> ProjectGraph<GraphPkg<'_>> {
    create_projects_graph(projects.iter().map(|project| GraphPkg { project }).collect(), &options)
        .graph
}

/// Apply one group of selectors (regular or `--filter-prod`) against the
/// already-built `graph` and return the selected project directories, in
/// selection order. `root_selector` is the optional
/// `{<workspace-root>}` selector appended to this pass. A pass with no
/// `filters` and no `root_selector` selects nothing.
fn filter_against<Pkg: BaseProject>(
    graph: &ProjectGraph<Pkg>,
    filters: &[String],
    root_selector: Option<&str>,
    follow_prod_deps_only: bool,
    prefix: &Path,
    walk_opts: &FilterWorkspaceProjectsOptions,
) -> miette::Result<Vec<PathBuf>> {
    if filters.is_empty() && root_selector.is_none() {
        return Ok(Vec::new());
    }
    let mut selectors: Vec<ProjectSelector> = filters
        .iter()
        .map(|filter| {
            let mut selector = parse_project_selector(filter, prefix);
            selector.follow_prod_deps_only = follow_prod_deps_only;
            selector
        })
        .collect();
    if let Some(root_selector) = root_selector {
        let mut selector = parse_project_selector(root_selector, prefix);
        selector.follow_prod_deps_only = follow_prod_deps_only;
        // pnpm generates this selector; the user did not write it. It has
        // to mean "the project whose directory is the workspace root",
        // which only glob matching says. Left to follow the pass,
        // `legacyDirFiltering`'s subtree matching would read it as "every
        // project below the root" and a recursive `run` / `exec` would
        // select the root alone.
        selector.use_glob_dir_filtering = Some(true);
        selectors.push(selector);
    }
    let selected = filter_workspace_projects(graph, &selectors, walk_opts)
        .map_err(miette::Report::new)
        .wrap_err("filtering workspace projects")?;
    Ok(selected.selected_projects)
}

/// Whether a recursive command drops the workspace root from an
/// all-exclusion (or unfiltered) `--filter` selection.
///
/// For `run` / `exec` (and `add` / `test`) a `!{<workspace-root>}`
/// selector is appended so a recursive `run` / `exec` skips the root
/// project unless it is explicitly included.
#[derive(Clone, Copy)]
pub enum AutoExcludeRoot<'a> {
    /// `run` / `exec` (also `add` / `test`): exclude the root when no
    /// inclusion selector is present and the workspace is not root-only.
    /// `workspace_patterns` is `config.workspacePackagePatterns`, used for
    /// the root-only guard.
    Enabled { workspace_patterns: Option<&'a [String]> },
    /// `pack` (and the other recursive commands): never auto-exclude.
    Disabled,
}

impl AutoExcludeRoot<'_> {
    /// The extra `{<workspace-root>}` selector to append to the
    /// `--filter` / `--filter-prod` selection, or `None` when no
    /// augmentation applies. [`select_recursive_projects`] routes it into
    /// the pass whose `follow_prod_deps_only` matches (the prod pass when
    /// a `--filter-prod` selector is present, else the regular pass).
    fn root_selector(&self, config: &Config, prefix: &Path) -> Option<String> {
        // pnpm pushes this inclusion onto the `--filter` list rather than
        // replacing it, and for every recursive command — so unlike the
        // exclusion below it is ungated, and it is additive.
        if config.workspace_root {
            return Some(format!("{{{}}}", relative_workspace_dir(config, prefix)));
        }
        let AutoExcludeRoot::Enabled { workspace_patterns } = self else {
            return None;
        };
        if config.include_workspace_root {
            return None;
        }
        // An inclusion selector already pins the selected set, so the
        // root is kept only if it matches one.
        if config
            .filter
            .iter()
            .chain(config.filter_prod.iter())
            .any(|filter| !filter.starts_with('!'))
        {
            return None;
        }
        // A root-only workspace (patterns === ['.']) has no non-root project
        // to keep, so excluding the root would empty the selection. Absent
        // patterns mean no `pnpm-workspace.yaml`.
        let patterns = (*workspace_patterns)?;
        if is_root_only_patterns(patterns) {
            return None;
        }
        Some(format!("!{{{}}}", relative_workspace_dir(config, prefix)))
    }
}

/// The workspace root as a path selectors can resolve against `prefix`,
/// which is where a `{<dir>}` selector is anchored.
fn relative_workspace_dir(config: &Config, prefix: &Path) -> String {
    let workspace_root = config.workspace_dir.as_deref().unwrap_or(prefix);
    pathdiff::diff_paths(workspace_root, prefix)
        .map(|path| path.to_string_lossy().into_owned())
        .filter(|path| !path.is_empty())
        .unwrap_or_else(|| ".".to_string())
}

/// Whether the workspace enumerates the root project only.
fn is_root_only_patterns(patterns: &[String]) -> bool {
    patterns.len() == 1 && patterns[0] == "."
}

/// `pnpm-exec-summary.json` top-level shape: `{ "executionStatus": { ... } }`.
#[derive(Serialize)]
struct ExecSummaryFile {
    #[serde(rename = "executionStatus")]
    execution_status: IndexMap<String, ExecutionStatus>,
}

/// One package's entry in the recursive summary. `duration` is in
/// milliseconds and present only once the action has run; `prefix` and
/// `message` are filled in for failures.
#[derive(Debug, Clone, Serialize)]
pub struct ExecutionStatus {
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl ExecutionStatus {
    pub fn queued() -> Self {
        ExecutionStatus { status: Status::Queued, duration: None, prefix: None, message: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Queued,
    Running,
    Passed,
    Skipped,
    Failure,
}

#[cfg(test)]
mod tests;
