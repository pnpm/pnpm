//! Shared machinery for the recursive (`-r`) variants of `run` and
//! `exec`: workspace-project discovery, `--filter` selection,
//! topological sorting, the `--resume-from` chunk trimming, and the
//! `pnpm-exec-summary.json` execution-status report.
//!
//! Ports the parts of pnpm's
//! [`exec.ts`](https://github.com/pnpm/pnpm/blob/8eb1be4988/exec/commands/src/exec.ts)
//! and
//! [`@pnpm/cli.utils`](https://github.com/pnpm/pnpm/blob/8eb1be4988/cli/utils/src/recursiveSummary.ts)
//! that `runRecursive` and the recursive `exec` handler both rely on.
//! The per-command pieces (which action runs per project, and the
//! command-specific error codes) live in `run/recursive.rs` and
//! `exec/recursive.rs`.

use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_package_manager::graph_sequencer;
use pacquet_package_manifest::DependencyGroup;
use pacquet_workspace::{
    FindWorkspaceProjectsOpts, Project, find_workspace_projects, read_workspace_manifest,
    workspace_package_patterns,
};
use pacquet_workspace_projects_filter::{FilterProjectsOptions, WorkspaceFilter, filter_projects};
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

/// Sort the workspace graph into topologically ordered chunks: every
/// project in chunk `i` depends only on projects in earlier chunks, so
/// chunk `i` may run after chunks `0..i`.
///
/// Port of pnpm's
/// [`sortProjects`](https://github.com/pnpm/pnpm/blob/8eb1be4988/workspace/projects-sorter/src/index.ts):
/// build a node → in-set-dependencies map (dropping self-edges and edges
/// leaving the selected set) and run it through [`graph_sequencer`].
pub fn sort_projects(graph: &ProjectGraph<GraphPkg<'_>>) -> Vec<Vec<PathBuf>> {
    let keys: Vec<PathBuf> = graph.keys().cloned().collect();
    let key_set: HashSet<&PathBuf> = keys.iter().collect();
    let dependency_graph: HashMap<PathBuf, Vec<PathBuf>> = graph
        .iter()
        .map(|(root, node)| {
            let dependencies = node
                .dependencies
                .iter()
                .filter(|dependency| *dependency != root && key_set.contains(dependency))
                .cloned()
                .collect();
            (root.clone(), dependencies)
        })
        .collect();
    graph_sequencer(&dependency_graph, &keys).chunks
}

/// Drop every chunk before the one containing the `resume_from` package,
/// so execution resumes from that package.
///
/// Port of pnpm's
/// [`getResumedPackageChunks`](https://github.com/pnpm/pnpm/blob/8eb1be4988/exec/commands/src/exec.ts#L100-L118):
/// the package is located by manifest name; an unknown name is a
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
/// Port of pnpm's
/// [`writeRecursiveSummary`](https://github.com/pnpm/pnpm/blob/8eb1be4988/exec/commands/src/exec.ts#L120-L124):
/// the per-package map is nested under an `executionStatus` key.
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
/// Port of pnpm's
/// [`throwOnCommandFail`](https://github.com/pnpm/pnpm/blob/8eb1be4988/cli/utils/src/recursiveSummary.ts#L28-L33);
/// the caller turns a non-zero count into its command-specific
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

/// Build the `--filter`-selected workspace graph the recursive command
/// runs over: the project graph restricted to the `config.filter` /
/// `config.filter_prod` selection (every project when no selector is
/// given). `prefix` is where path selectors resolve. `auto_exclude_root`
/// controls the main-dispatch auto-exclusion of the workspace root for
/// `run` / `exec`.
///
/// Ports pnpm's `selectedProjectsGraph` main-dispatch step
/// (<https://github.com/pnpm/pnpm/blob/8eb1be4988/pnpm/src/main.ts#L238-L323>),
/// including the `!{<workspace-root>}` augmentation.
pub fn select_recursive_projects<'a>(
    projects: &'a [Project],
    config: &Config,
    prefix: &Path,
    auto_exclude_root: AutoExcludeRoot<'_>,
) -> miette::Result<ProjectGraph<GraphPkg<'a>>> {
    // Filter against the same graph options the sort uses, so the selected
    // set and the topological order agree on edges.
    let graph_options = CreateProjectsGraphOptions::default();
    let mut graph = create_projects_graph(
        projects.iter().map(|project| GraphPkg { project }).collect(),
        &graph_options,
    )
    .graph;

    let mut filters: Vec<WorkspaceFilter> =
        config
            .filter
            .iter()
            .map(|filter| WorkspaceFilter { filter: filter.clone(), follow_prod_deps_only: false })
            .chain(config.filter_prod.iter().map(|filter| WorkspaceFilter {
                filter: filter.clone(),
                follow_prod_deps_only: true,
            }))
            .collect();
    if let Some(root_exclusion) = auto_exclude_root.root_exclusion(config, prefix, &filters) {
        filters.push(root_exclusion);
    }

    // No selector (and no auto-exclusion) selects every project.
    if filters.is_empty() {
        return Ok(graph);
    }

    let selected = filter_projects(
        projects.iter().map(|project| GraphPkg { project }).collect(),
        &filters,
        &FilterProjectsOptions {
            prefix: prefix.to_path_buf(),
            link_workspace_packages: graph_options.link_workspace_packages,
            // pnpm's main dispatch filters with `useGlobDirFiltering:
            // !config.legacyDirFiltering`, whose default is glob
            // matching. It is load-bearing for the `!{<workspace-root>}`
            // augmentation: glob matching excludes only the project whose
            // dir equals the workspace root, whereas the legacy
            // subtree match would also drop every nested package.
            // `legacyDirFiltering` is not surfaced by `Config` yet, so
            // this stays at pnpm's default.
            use_glob_dir_filtering: true,
        },
    )
    .map_err(miette::Report::new)
    .wrap_err("filtering workspace projects")?;

    Ok(selected
        .selected_projects
        .iter()
        .filter_map(|root| graph.swap_remove(root).map(|node| (root.clone(), node)))
        .collect())
}

/// Whether a recursive command drops the workspace root from an
/// all-exclusion (or unfiltered) `--filter` selection.
///
/// Mirrors the `cmd === 'run' || cmd === 'exec' || cmd === 'add' ||
/// cmd === 'test'` arm of pnpm's main dispatch
/// (<https://github.com/pnpm/pnpm/blob/8eb1be4988/pnpm/src/main.ts#L251-L259>),
/// which appends a `!{<workspace-root>}` selector so a recursive
/// `run` / `exec` skips the root project unless it is explicitly
/// included.
#[derive(Clone, Copy)]
pub enum AutoExcludeRoot<'a> {
    /// `run` / `exec` (upstream also `add` / `test`): exclude the root
    /// when no inclusion selector is present and the workspace is not
    /// root-only. `workspace_patterns` is
    /// `config.workspacePackagePatterns`, used for the root-only guard.
    Enabled { workspace_patterns: Option<&'a [String]> },
    /// `pack` (upstream's other recursive commands): never auto-exclude.
    Disabled,
}

impl AutoExcludeRoot<'_> {
    /// The `!{<workspace-root>}` selector to append to `existing` (the
    /// filters built from `--filter` / `--filter-prod`), or `None` when
    /// the augmentation does not apply.
    fn root_exclusion(
        &self,
        config: &Config,
        prefix: &Path,
        existing: &[WorkspaceFilter],
    ) -> Option<WorkspaceFilter> {
        let AutoExcludeRoot::Enabled { workspace_patterns } = self else {
            return None;
        };
        // An inclusion selector already pins the selected set, so the
        // root is kept only if it matches one. Mirrors upstream's
        // `!filters.some(({ filter }) => !filter.startsWith('!'))`.
        if existing.iter().any(|filter| !filter.filter.starts_with('!')) {
            return None;
        }
        // A root-only workspace (patterns === ['.']) has no non-root
        // project to keep, so excluding the root would empty the
        // selection. Absent patterns mean no `pnpm-workspace.yaml`.
        let patterns = (*workspace_patterns)?;
        if is_root_only_patterns(patterns) {
            return None;
        }
        let workspace_root = config.workspace_dir.as_deref().unwrap_or(prefix);
        let relative = pathdiff::diff_paths(workspace_root, prefix)
            .map(|path| path.to_string_lossy().into_owned())
            .filter(|path| !path.is_empty())
            .unwrap_or_else(|| ".".to_string());
        Some(WorkspaceFilter {
            filter: format!("!{{{relative}}}"),
            follow_prod_deps_only: !config.filter_prod.is_empty(),
        })
    }
}

/// Port of pnpm's
/// [`isRootOnlyPatterns`](https://github.com/pnpm/pnpm/blob/8eb1be4988/pnpm/src/main.ts#L40-L42):
/// the workspace enumerates the root project only.
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
        // Precedence mirrors upstream's `createNode` spread: peer, then
        // dev (unless excluded), then optional, then prod, with a later
        // group overwriting an earlier duplicate's specifier while
        // keeping the first-seen position.
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
