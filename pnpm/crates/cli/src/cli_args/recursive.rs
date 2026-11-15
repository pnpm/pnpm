//! Shared machinery for the recursive (`-r`) variants of `run` and
//! `exec`: workspace-project discovery, `--filter` selection,
//! dependency graph construction, `--resume-from` trimming, and the
//! `pnpm-exec-summary.json` execution-status report.
//!
//! The per-command pieces (which action runs per project, and the
//! command-specific error codes) live in `run/recursive.rs` and
//! `exec/recursive.rs`.

pub use dependencies::{filtered_projects_dependencies, projects_to_verify, sequence_graph};
pub use execution_args::RecursiveExecutionArgs;
pub use importer_selection::{selected_workspace_importer_ids, selectors_narrow_the_run};
pub use summary::{ExecutionStatus, Status, count_failures, write_recursive_summary};
pub use unmatched::UnmatchedFilters;

mod dependencies;
mod execution_args;
mod importer_selection;
mod unmatched;

use crate::{
    cli_args::catalogs::{configured_catalogs, workspace_catalogs},
    state::{ProjectHost, host_requirements},
};
use derive_more::{Display, Error};
use indexmap::IndexMap;
use miette::{Context, Diagnostic, IntoDiagnostic};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::{Config, FilterSource, LinkWorkspacePackages};
use pnpm_reporter::LogEvent;
use pnpm_workspace::{
    FindWorkspaceProjectsOpts, GraphPkg, Project, find_workspace_projects,
    importer_id_from_root_dir, read_workspace_manifest, workspace_package_patterns,
};
use pnpm_workspace_projects_filter::{
    FilterWorkspaceProjectsOptions, FilteredGraphProjects, ProjectSelector,
    filter_workspace_projects_with_prod_graph, parse_project_selector,
};
use pnpm_workspace_projects_graph::{
    BaseProject, CreateProjectsGraphOptions, ProjectGraph, create_projects_graph,
};
use serde::Serialize;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use unmatched::unmatched_filters;

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
/// [`UnmatchedFilters::report`].
#[derive(Debug, Display, Error, Diagnostic)]
#[diagnostic(code(ERR_PNPM_NO_MATCHING_PROJECTS))]
pub struct NoMatchingProjects {
    #[error(not(source))]
    pub message: String,
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

/// Enumerate the projects of the workspace rooted at `workspace_root`,
/// returning them alongside the package patterns that selected them.
/// Shared by recursive `run` / `exec` / `pack` so all discover the same
/// set before [`select_recursive_projects`] narrows it. The patterns feed
/// [`keeps_workspace_root`]; `None` means no `pnpm-workspace.yaml` was
/// found or `--ignore-workspace` is set.
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
        &FindWorkspaceProjectsOpts {
            patterns: patterns.clone(),
            ignored_directories: config.managed_directories(),
        },
    )
    .wrap_err("finding workspace projects")?;
    Ok((projects, patterns))
}

/// Check what every project of `selection`'s workspace, the root project
/// included, requires of the host, per [`ProjectHost`], before a recursive
/// command runs code in them. No install-family root check runs before
/// these commands, so the root project is not skipped.
pub(crate) fn check_workspace_requirements(
    selection: &RecursiveSelection<'_>,
    config: &Config,
    workspace_root: &Path,
    emit: fn(&LogEvent),
) -> miette::Result<()> {
    let projects = selection
        .full_graph()
        .values()
        .map(|node| node.package.project);
    let host = ProjectHost::new(config, workspace_root);
    for warning in host.check_every_project(&host_requirements(
        projects,
        config.runtime_on_fail.map(pnpm_config::RuntimeOnFail::as_str),
    ))? {
        emit(&warning);
    }
    Ok(())
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
    let (selection, unmatched) =
        select_recursive_projects_deferring_no_match(projects, config, prefix, auto_exclude_root)?;
    match unmatched {
        Some(unmatched) => Err(unmatched.report()),
        None => Ok(selection),
    }
}

/// [`select_recursive_projects`], with a `--fail-if-no-match` selection
/// that came back empty returned instead of raised. An install that has a
/// non-npm ecosystem enabled decides after that ecosystem resolved its own
/// selection: a `--filter` selector can name a project only that ecosystem
/// knows about.
pub fn select_recursive_projects_deferring_no_match<'a>(
    projects: &'a [Project],
    config: &Config,
    prefix: &Path,
    auto_exclude_root: AutoExcludeRoot<'_>,
) -> miette::Result<(RecursiveSelection<'a>, Option<UnmatchedFilters>)> {
    let catalogs = configured_catalogs(config)?;
    let graph_options = recursive_graph_options(config, &catalogs);
    let all = build_graph(projects, graph_options);

    let root_selector = auto_exclude_root.root_selector(config, prefix);

    if config.filter.is_empty() && config.filter_prod.is_empty() && root_selector.is_none() {
        return Ok(unnarrowed_selection(all, config, prefix));
    }

    let prod_all = production_filter_graph(projects, config, graph_options);
    let walk_opts = recursive_filter_options(config, prefix);
    let selectors = parse_configured_selectors(config, root_selector.as_deref(), prefix);
    let filtered = filter_graphs(&all, prod_all.as_ref(), &selectors, &walk_opts)?;
    let selected = build_selected_graph(
        &all,
        prod_all.as_ref(),
        &filtered.selected_projects,
        &filtered.prod_only_projects,
    );

    let unmatched = unmatched_filters(selected.len(), all.len(), config, prefix);
    Ok((
        RecursiveSelection {
            selected,
            all: Some(all),
            prod_all,
            prod_only_selected: filtered.prod_only_projects,
        },
        unmatched,
    ))
}

/// Every project, for a run no selector narrowed. `all` already is the full
/// graph, so the sort has nothing to resolve edges through.
fn unnarrowed_selection<'a>(
    all: ProjectGraph<GraphPkg<'a>>,
    config: &Config,
    prefix: &Path,
) -> (RecursiveSelection<'a>, Option<UnmatchedFilters>) {
    let unmatched = unmatched_filters(all.len(), all.len(), config, prefix);
    let selection = RecursiveSelection {
        selected: all,
        all: None,
        prod_all: None,
        prod_only_selected: HashSet::new(),
    };
    (selection, unmatched)
}

/// Assemble the selected graph in `selected_projects` order, taking prod-pruned
/// edges for `prod_only_selected` entries and full edges otherwise.
fn build_selected_graph<'a>(
    all: &ProjectGraph<GraphPkg<'a>>,
    prod_all: Option<&ProjectGraph<GraphPkg<'a>>>,
    selected_projects: &[PathBuf],
    prod_only_selected: &HashSet<PathBuf>,
) -> ProjectGraph<GraphPkg<'a>> {
    let mut selected = ProjectGraph::new();
    for dir in selected_projects {
        let source_graph = prod_all
            .filter(|_| prod_only_selected.contains(dir))
            .unwrap_or(all);
        if let Some(node) = source_graph.get(dir) {
            selected.insert(dir.clone(), node.clone());
        }
    }
    selected
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

/// Print [`no_projects_matched_message`] for a command invoked in `prefix`.
pub fn print_no_projects_matched(config: &Config, prefix: &Path) {
    pnpm_diagnostics::println!(
        "{}",
        no_projects_matched_message(notice_workspace_dir(config, prefix)),
    );
}

/// The lockfile importer ids of `selection`'s projects, in selection
/// order. The ids key the lockfile's `importers` map, so a lockfile-driven
/// command (`licenses`, `sbom`) can narrow its walk to what `--filter` /
/// `--filter-prod` selected.
pub fn selected_importer_ids(
    selection: &RecursiveSelection<'_>,
    lockfile_dir: &Path,
) -> Vec<String> {
    selection.selected
        .keys()
        .map(|project_dir| importer_id_from_root_dir(lockfile_dir, project_dir))
        .collect()
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

/// Parse `config`'s `--filter` and `--filter-prod` entries in evaluation order,
/// appending `root_selector` (if any) with glob directory matching.
pub fn parse_configured_selectors(
    config: &Config,
    root_selector: Option<&str>,
    prefix: &Path,
) -> Vec<ProjectSelector> {
    let mut selectors: Vec<ProjectSelector> = config
        .ordered_filters()
        .into_iter()
        .map(|(source, filter)| {
            let mut selector = parse_project_selector(filter, prefix);
            selector.follow_prod_deps_only = source == FilterSource::FilterProd;
            selector
        })
        .collect();
    if let Some(root_selector) = root_selector {
        let mut selector = parse_project_selector(root_selector, prefix);
        selector.follow_prod_deps_only = !config.filter_prod.is_empty();
        // pnpm generates this selector; the user did not write it. It has
        // to mean "the project whose directory is the workspace root",
        // which only glob matching says. Left to follow the pass,
        // `legacyDirFiltering`'s subtree matching would read it as "every
        // project below the root" and a recursive `run` / `exec` would
        // select the root alone.
        selector.use_glob_dir_filtering = Some(true);
        selectors.push(selector);
    }
    selectors
}

/// Evaluate `selectors` across `all_graph` and `prod_graph` in one
/// polarity-chunked pass. An empty `selectors` slice selects nothing.
pub fn filter_graphs<Pkg: BaseProject>(
    all_graph: &ProjectGraph<Pkg>,
    prod_graph: Option<&ProjectGraph<Pkg>>,
    selectors: &[ProjectSelector],
    walk_opts: &FilterWorkspaceProjectsOptions,
) -> miette::Result<FilteredGraphProjects> {
    if selectors.is_empty() {
        return Ok(FilteredGraphProjects::default());
    }
    filter_workspace_projects_with_prod_graph(all_graph, prod_graph, selectors, walk_opts)
        .map_err(miette::Report::new)
        .wrap_err("filtering workspace projects")
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
    pub fn root_selector(&self, config: &Config, prefix: &Path) -> Option<String> {
        // pnpm pushes this inclusion onto the `--filter` list rather than
        // replacing it, and for every recursive command — so unlike the
        // exclusion below it is ungated, and it is additive.
        if config.workspace_root {
            return Some(format!("{{{}}}", relative_workspace_dir(config, prefix)));
        }
        let AutoExcludeRoot::Enabled { workspace_patterns } = self else {
            return None;
        };
        if keeps_workspace_root(config, *workspace_patterns) {
            return None;
        }
        // An inclusion selector already pins the selected set, so the
        // root is kept only if it matches one.
        if config.filter
            .iter()
            .chain(config.filter_prod.iter())
            .any(|filter| !filter.starts_with('!'))
        {
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

/// Whether a selection that names no project keeps the workspace root. It
/// does when any of these hold:
///
/// - `includeWorkspaceRoot` is set.
/// - `workspace_patterns` lists only the root, so excluding it would empty
///   the selection.
/// - `workspace_patterns` is `None`: there is no `pnpm-workspace.yaml`, or
///   `--ignore-workspace` is set and the root is kept alongside every
///   nested project.
pub(crate) fn keeps_workspace_root(config: &Config, workspace_patterns: Option<&[String]>) -> bool {
    config.include_workspace_root || workspace_patterns.is_none_or(is_root_only_patterns)
}

/// Whether the workspace enumerates the root project only.
fn is_root_only_patterns(patterns: &[String]) -> bool {
    patterns.len() == 1 && patterns[0] == "."
}

/// The current lockfile install records for `project` when it has a dedicated
/// lockfile.
pub(crate) fn load_project_current_lockfile(
    config: &Config,
    project: &pnpm_workspace::Project,
) -> Result<Option<pnpm_lockfile::Lockfile>, pnpm_lockfile::LoadLockfileError> {
    let mut project_config = config.clone();
    let project_name = super::pipelines::project_manifest_name(project);
    project_config.anchor_dedicated_project(&project.root_dir, project_name);
    pnpm_lockfile::Lockfile::load_current_from_install_state_dir(&project_config.install_state_dir)
}

#[cfg(test)]
mod tests;

/// User directory selectors follow the configured legacy/glob mode; generated
/// root-exclusion selectors choose glob matching in the filtering helper.
pub fn recursive_filter_options(config: &Config, prefix: &Path) -> FilterWorkspaceProjectsOptions {
    FilterWorkspaceProjectsOptions {
        // The mode user-written `{<dir>}` selectors match in. The
        // generated `!{<workspace-root>}` selector pins itself to glob
        // matching instead — see `parse_configured_selectors`.
        use_glob_dir_filtering: !config.legacy_dir_filtering,
        workspace_dir: config.workspace_dir
            .as_deref()
            .unwrap_or(prefix)
            .to_path_buf(),
        test_pattern: config.test_pattern.clone(),
        changed_files_ignore_pattern: config.changed_files_ignore_pattern.clone(),
    }
}

fn production_filter_graph<'a>(
    projects: &'a [Project],
    config: &Config,
    graph_options: CreateProjectsGraphOptions<'_>,
) -> Option<ProjectGraph<GraphPkg<'a>>> {
    if config.filter_prod.is_empty() {
        None
    } else {
        Some(build_graph(
            projects,
            CreateProjectsGraphOptions { ignore_dev_deps: true, ..graph_options },
        ))
    }
}

/// Respect the configured linking policy when determining workspace edges.
fn recursive_graph_options<'a>(
    config: &'a Config,
    catalogs: &'a Catalogs,
) -> CreateProjectsGraphOptions<'a> {
    CreateProjectsGraphOptions {
        link_workspace_packages: Some(config.link_workspace_packages != LinkWorkspacePackages::Off),
        catalogs: workspace_catalogs(config, catalogs),
        ..CreateProjectsGraphOptions::default()
    }
}

mod summary;
