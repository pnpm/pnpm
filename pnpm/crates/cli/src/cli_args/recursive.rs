//! Shared machinery for the recursive (`-r`) variants of `run` and
//! `exec`: workspace-project discovery, `--filter` selection,
//! topological sorting, the `--resume-from` chunk trimming, and the
//! `pnpm-exec-summary.json` execution-status report.
//!
//! The per-command pieces (which action runs per project, and the
//! command-specific error codes) live in `run/recursive.rs` and
//! `exec/recursive.rs`.

use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::{Config, LinkWorkspacePackages};
use pacquet_package_manager::{GraphSequencerResult, graph_sequencer};
use pacquet_package_manifest::DependencyGroup;
use pacquet_workspace::{
    FindWorkspaceProjectsOpts, Project, find_workspace_projects, read_workspace_manifest,
    workspace_package_patterns,
};
use pacquet_workspace_projects_filter::{
    FilterWorkspaceProjectsOptions, ProjectSelector, filter_workspace_projects,
    parse_project_selector,
};
use pacquet_workspace_projects_graph::{
    BaseProject, CreateProjectsGraphOptions, GraphProject, ProjectGraph, create_projects_graph,
};
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

/// Sort the `--filter`-selected workspace projects into topologically
/// ordered chunks: every project in chunk `i` depends only on projects in
/// earlier chunks, so chunk `i` may run after chunks `0..i`.
///
/// Order is resolved through the full workspace graph, so two selected
/// projects connected only through an unselected one are still ordered
/// correctly. `--filter-prod` projects in `prod_only_selected` resolve
/// through `prod_all` instead, so the dev edges that selection pruned are
/// not pulled back into the order. Unrelated projects share a chunk and
/// stay concurrent.
pub fn sort_filtered_projects<Pkg>(
    selected: &ProjectGraph<Pkg>,
    all: &ProjectGraph<Pkg>,
    prod_all: Option<&ProjectGraph<Pkg>>,
    prod_only_selected: &HashSet<PathBuf>,
) -> Vec<Vec<PathBuf>> {
    match prod_all {
        None => sort_projects(selected, Some(all)),
        Some(prod_all) => {
            sequence_graph_by_project(selected, |project_dir| {
                if prod_only_selected.contains(project_dir) { prod_all } else { all }
            })
            .chunks
        }
    }
}

/// Sort `graph` into topologically ordered chunks: every project in chunk
/// `i` depends only on projects in earlier chunks, so chunk `i` may run
/// after chunks `0..i`.
///
/// `full_graph` resolves edges that pass through projects outside `graph`,
/// so two selected projects connected only through an unselected one are
/// still ordered correctly; `None` resolves only the edges among `graph`'s
/// own projects.
pub fn sort_projects<Pkg>(
    graph: &ProjectGraph<Pkg>,
    full_graph: Option<&ProjectGraph<Pkg>>,
) -> Vec<Vec<PathBuf>> {
    sequence_graph(graph, full_graph.unwrap_or(graph)).chunks
}

/// Sequence `projects_graph` into topologically ordered chunks, resolving
/// transitive edges through `full_projects_graph`. See [`sort_projects`].
fn sequence_graph<Pkg>(
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
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut stack: Vec<PathBuf> =
        projects_graph.get(project_dir).map(|node| node.dependencies.clone()).unwrap_or_default();
    while let Some(dependency_dir) = stack.pop() {
        if dependency_dir.as_path() == project_dir || !visited.insert(dependency_dir.clone()) {
            continue;
        }
        if sorted.contains(dependency_dir.as_path()) {
            dependencies.push(dependency_dir);
        } else if let Some(node) = full_projects_graph.get(&dependency_dir) {
            stack.extend(node.dependencies.iter().cloned());
        }
    }
    dependencies
}

/// Drop every chunk before the one containing the `resume_from` package,
/// so execution resumes from that package.
///
/// The package is located by manifest name; an unknown name is a
/// [`ResumeFromNotFound`] error.
pub fn get_resumed_package_chunks(
    resume_from: &str,
    chunks: Vec<Vec<PathBuf>>,
    graph: &ProjectGraph<GraphPkg<'_>>,
) -> Result<Vec<Vec<PathBuf>>, ResumeFromNotFound> {
    let resume_root = graph
        .iter()
        .find(|(_, node)| node.package.manifest_name() == Some(resume_from))
        .map(|(root, _)| root.clone())
        .ok_or_else(|| ResumeFromNotFound { resume_from: resume_from.to_string() })?;
    let position = chunks
        .iter()
        .position(|chunk| chunk.contains(&resume_root))
        .expect("the resume-from package is present in the sorted chunks");
    Ok(chunks.into_iter().skip(position).collect())
}

/// Write the recursive summary to `pnpm-exec-summary.json` under `dir`.
///
/// The per-package map is nested under an `executionStatus` key.
pub fn write_recursive_summary(
    dir: &Path,
    summary: &IndexMap<PathBuf, ExecutionStatus>,
) -> miette::Result<()> {
    let execution_status = summary
        .iter()
        .map(|(root, status)| (root.to_string_lossy().into_owned(), status.clone()))
        .collect();
    let path = dir.join("pnpm-exec-summary.json");
    let mut contents =
        serde_json::to_string_pretty(&ExecSummaryFile { execution_status }).into_diagnostic()?;
    contents.push('\n');
    std::fs::write(&path, contents)
        .into_diagnostic()
        .wrap_err_with(|| format!("writing {}", path.display()))
}

/// Count the packages whose action failed.
///
/// The caller turns a non-zero count into its command-specific
/// `ERR_PNPM_RECURSIVE_FAIL` error.
pub fn count_failures(summary: &IndexMap<PathBuf, ExecutionStatus>) -> usize {
    summary.values().filter(|status| status.status == Status::Failure).count()
}

/// Read the workspace manifest at `workspace_root` and enumerate its
/// projects, returning them alongside the workspace package patterns
/// (`config.workspacePackagePatterns`). Shared by recursive `run` /
/// `exec` / `pack` so all discover the same set before
/// [`select_recursive_projects`] narrows it. The patterns feed the
/// root-only guard of [`AutoExcludeRoot`]; `None` means no
/// `pnpm-workspace.yaml` was found.
pub fn discover_workspace_projects(
    workspace_root: &Path,
) -> miette::Result<(Vec<Project>, Option<Vec<String>>)> {
    let patterns = read_workspace_manifest(workspace_root)
        .into_diagnostic()
        .wrap_err("reading pnpm-workspace.yaml")?
        .map(|manifest| workspace_package_patterns(&manifest));
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
/// runs over, together with the graphs [`sort_filtered_projects`] resolves
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

    // The main-dispatch `!{<workspace-root>}` selector for `run` / `exec` drops
    // the workspace root from an unfiltered or all-exclusion selection. It
    // routes into the selection pass whose `follow_prod_deps_only` matches: the
    // prod pass when a `--filter-prod` selector is present, otherwise the
    // regular pass.
    let root_exclusion = auto_exclude_root.root_exclusion(config, prefix);

    if config.filter.is_empty() && config.filter_prod.is_empty() && root_exclusion.is_none() {
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
        // Glob dir filtering (the default, the inverse of
        // `legacyDirFiltering`) is load-bearing for the
        // `!{<workspace-root>}` augmentation: glob matching excludes only
        // the project whose dir equals the workspace root, whereas the
        // legacy subtree match would also drop every nested package.
        // `legacyDirFiltering` is not surfaced by `Config` yet, so this
        // stays at the default.
        use_glob_dir_filtering: true,
        workspace_dir: config.workspace_dir.as_deref().unwrap_or(prefix).to_path_buf(),
        test_pattern: config.test_pattern.clone(),
        changed_files_ignore_pattern: config.changed_files_ignore_pattern.clone(),
    };
    let regular_selected = filter_against(
        &all,
        &config.filter,
        root_exclusion.as_deref().filter(|_| !root_in_prod),
        false,
        prefix,
        &walk_opts,
    )?;
    let prod_selected = match &prod_all {
        Some(prod_all) => filter_against(
            prod_all,
            &config.filter_prod,
            root_exclusion.as_deref().filter(|_| root_in_prod),
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
    // recursive runners execute and print a topological chunk's projects in it.
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

    Ok(RecursiveSelection { selected, all: Some(all), prod_all, prod_only_selected })
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
/// selection order. `root_exclusion` is the optional
/// `!{<workspace-root>}` selector appended to this pass. A pass with no
/// `filters` and no `root_exclusion` selects nothing.
fn filter_against<Pkg: BaseProject>(
    graph: &ProjectGraph<Pkg>,
    filters: &[String],
    root_exclusion: Option<&str>,
    follow_prod_deps_only: bool,
    prefix: &Path,
    walk_opts: &FilterWorkspaceProjectsOptions,
) -> miette::Result<Vec<PathBuf>> {
    if filters.is_empty() && root_exclusion.is_none() {
        return Ok(Vec::new());
    }
    let selectors: Vec<ProjectSelector> = filters
        .iter()
        .map(String::as_str)
        .chain(root_exclusion)
        .map(|filter| {
            let mut selector = parse_project_selector(filter, prefix);
            selector.follow_prod_deps_only = follow_prod_deps_only;
            selector
        })
        .collect();
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
    /// The `!{<workspace-root>}` selector to append to the `--filter` /
    /// `--filter-prod` selection, or `None` when the augmentation does not
    /// apply. [`select_recursive_projects`] routes it into the pass whose
    /// `follow_prod_deps_only` matches (the prod pass when a `--filter-prod`
    /// selector is present, else the regular pass).
    fn root_exclusion(&self, config: &Config, prefix: &Path) -> Option<String> {
        let AutoExcludeRoot::Enabled { workspace_patterns } = self else {
            return None;
        };
        // pnpm additionally suppresses the exclusion under
        // `--include-workspace-root` and, for `--workspace-root`, pushes
        // an inclusion `{<root>}` filter instead. pacquet surfaces
        // neither flag yet, so only this exclusion arm applies.
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
        let workspace_root = config.workspace_dir.as_deref().unwrap_or(prefix);
        let relative = pathdiff::diff_paths(workspace_root, prefix)
            .map(|path| path.to_string_lossy().into_owned())
            .filter(|path| !path.is_empty())
            .unwrap_or_else(|| ".".to_string());
        Some(format!("!{{{relative}}}"))
    }
}

/// Whether the workspace enumerates the root project only.
fn is_root_only_patterns(patterns: &[String]) -> bool {
    patterns.len() == 1 && patterns[0] == "."
}

/// Adapter that lets a [`Project`] feed `create_projects_graph`. Owns
/// nothing beyond a borrow of the project; the graph reads the manifest
/// name, version, and dependency groups through it.
#[derive(Clone, Copy)]
pub struct GraphPkg<'a> {
    pub project: &'a Project,
}

impl BaseProject for GraphPkg<'_> {
    fn root_dir(&self) -> &Path {
        &self.project.root_dir
    }

    fn manifest_name(&self) -> Option<&str> {
        self.project.manifest.value().get("name").and_then(|name| name.as_str())
    }
}

impl GraphProject for GraphPkg<'_> {
    fn manifest_version(&self) -> Option<&str> {
        self.project.manifest.value().get("version").and_then(|version| version.as_str())
    }

    fn merged_dependencies(&self, ignore_dev_deps: bool) -> Vec<(String, String)> {
        // Precedence: peer, then dev (unless excluded), then optional,
        // then prod, with a later group overwriting an earlier
        // duplicate's specifier while keeping the first-seen position.
        let mut merged: IndexMap<String, String> = IndexMap::new();
        let mut absorb = |group: DependencyGroup| {
            for (name, spec) in self.project.manifest.dependencies([group]) {
                merged.insert(name.to_string(), spec.to_string());
            }
        };
        absorb(DependencyGroup::Peer);
        if !ignore_dev_deps {
            absorb(DependencyGroup::Dev);
        }
        absorb(DependencyGroup::Optional);
        absorb(DependencyGroup::Prod);
        merged.into_iter().collect()
    }
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
