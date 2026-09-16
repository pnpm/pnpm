//! Which workspace projects an install-family command acts on: the
//! `--filter` / `-r` selection, and how it is dispatched.

use super::{
    DedicatedProjects, InstallFamilyPlan, InstallFamilySelection, configuration,
    configuration::apply_runtime_on_fail, precomputed_workspace_cycles, project_dependencies,
    sequence_project_dependencies,
};
use crate::cli_args::recursive::{
    AutoExcludeRoot, RecursiveSelection, UnmatchedFilters, discover_workspace_projects,
    select_recursive_projects_deferring_no_match,
};
use indexmap::IndexMap;
use pnpm_config::Config;
use pnpm_reporter::{LogEvent, LogLevel, Reporter, ScopeLog};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

/// A dispatched install-family selection: how to run it, what the other
/// ecosystems narrow themselves by, and whether the selectors matched no
/// npm project at all.
pub(crate) struct InstallFamily {
    pub(crate) plan: InstallFamilyPlan,
    pub(crate) scope: Option<WorkspaceScope>,
    pub(crate) unmatched: Option<UnmatchedFilters>,
}

/// What a `--filter` selection resolved to among the npm workspace
/// projects.
///
/// The other ecosystems narrow themselves by it: a project of theirs that
/// shares a directory with one of `projects` is installed exactly when that
/// project is selected, which is the only identity the workspace knows that
/// directory by. One in a directory of its own is selected by its own name
/// and path instead.
#[derive(Clone)]
pub(crate) struct WorkspaceScope {
    pub(crate) projects: Arc<HashSet<PathBuf>>,
    pub(crate) selected: Arc<HashSet<PathBuf>>,
}

pub(super) fn select_install_family_plan<Reporter: self::Reporter>(
    cfg: &Config,
    prefix: &Path,
    manifest_path: &Path,
    recursive_sort: bool,
    auto_exclude_root: bool,
    precompute_workspace_cycles: bool,
) -> miette::Result<InstallFamilyPlan> {
    let family = select_install_family::<Reporter>(
        cfg,
        prefix,
        manifest_path,
        recursive_sort,
        auto_exclude_root,
        precompute_workspace_cycles,
    )?;
    match family.unmatched {
        Some(unmatched) => Err(unmatched.report()),
        None => Ok(family.plan),
    }
}

/// [`select_install_family_plan`], with the workspace scope the other
/// ecosystems narrow themselves by and an empty `--filter` selection left
/// for the caller to resolve.
pub(super) fn select_install_family<Reporter: self::Reporter>(
    cfg: &Config,
    prefix: &Path,
    manifest_path: &Path,
    recursive_sort: bool,
    auto_exclude_root: bool,
    precompute_workspace_cycles: bool,
) -> miette::Result<InstallFamily> {
    let Some((selection, unmatched)) = select_workspace_projects_with_cycles(
        cfg,
        prefix,
        manifest_path,
        recursive_sort,
        auto_exclude_root,
        precompute_workspace_cycles,
    )?
    else {
        return Ok(InstallFamily { plan: InstallFamilyPlan::Single, scope: None, unmatched: None });
    };
    // Only an install another ecosystem takes part in reads the scope. The
    // npm install reads the selection itself, so it pays nothing for this.
    let scope = crate::ecosystem_install::is_enabled(cfg)
        .then(|| WorkspaceScope {
            projects: Arc::new(
                selection.projects
                    .iter()
                    .map(|project| project.root_dir.clone())
                    .collect(),
            ),
            selected: Arc::clone(&selection.selected_dirs),
        });
    // Report what the `--filter` / `-r` selection resolved to, so the user
    // can confirm it before the install acts on it. Emitted once here for
    // every plan shape below — a `PerProject` plan installs each selected
    // project separately, and those child installs must not each report
    // the workspace again. The unnarrowed install reports its own scope
    // from inside the installer, where the workspace walk it already does
    // supplies the count.
    Reporter::emit(&LogEvent::Scope(ScopeLog {
        level: LogLevel::Debug,
        selected: selection.selected_dirs.len(),
        total: Some(selection.projects.len()),
        workspace_prefix: Some(selection.workspace_root.to_string_lossy().into_owned()),
    }));
    let plan = if cfg.shares_one_lockfile() {
        InstallFamilyPlan::Shared(Box::new(selection))
    } else {
        InstallFamilyPlan::PerProject(DedicatedProjects::new(cfg, selection))
    };
    Ok(InstallFamily { plan, scope, unmatched })
}

pub(crate) fn select_workspace_projects(
    cfg: &Config,
    prefix: &Path,
    manifest_path: &Path,
    recursive_sort: bool,
    auto_exclude_root: bool,
) -> miette::Result<Option<InstallFamilySelection>> {
    let selected = select_workspace_projects_with_cycles(
        cfg,
        prefix,
        manifest_path,
        recursive_sort,
        auto_exclude_root,
        false,
    )?;
    match selected {
        Some((_, Some(unmatched))) => Err(unmatched.report()),
        Some((selection, None)) => Ok(Some(selection)),
        None => Ok(None),
    }
}

fn select_workspace_projects_with_cycles(
    cfg: &Config,
    prefix: &Path,
    manifest_path: &Path,
    recursive_sort: bool,
    auto_exclude_root: bool,
    precompute_workspace_cycles: bool,
) -> miette::Result<Option<(InstallFamilySelection, Option<UnmatchedFilters>)>> {
    if !cfg.recursive {
        return Ok(None);
    }

    let workspace_root = cfg.workspace_dir.clone().unwrap_or_else(|| prefix.to_path_buf());
    let (mut projects, workspace_patterns) = discover_workspace_projects(&workspace_root, cfg)?;
    apply_runtime_on_fail(cfg, &mut projects);
    let resolved = resolve_selection(
        &projects,
        cfg,
        prefix,
        if auto_exclude_root {
            AutoExcludeRoot::Enabled { workspace_patterns: workspace_patterns.as_deref() }
        } else {
            AutoExcludeRoot::Disabled
        },
        (recursive_sort, precompute_workspace_cycles),
    )?;

    let active_dir = manifest_path.parent().expect("manifest path always has a parent dir");
    let active_manifest_is_standin =
        configuration::active_manifest_is_standin(active_dir, &projects)?;
    let install_dirs = install_dirs(&resolved.selected_dirs, &projects, &workspace_root);

    Ok(Some((
        InstallFamilySelection {
            workspace_root,
            projects,
            project_dependencies: resolved.project_dependencies,
            ordered_dirs: resolved.ordered_dirs,
            selected_dirs: resolved.selected_dirs,
            install_dirs: Arc::new(install_dirs),
            active_manifest_is_standin,
            workspace_cycles: resolved.workspace_cycles,
        },
        resolved.unmatched,
    )))
}

/// What the `--filter` selection resolved to: the selected projects' edges,
/// their build order, their directories, the cycles among them, and the
/// empty-selection failure it earns.
struct ResolvedSelection {
    project_dependencies: IndexMap<PathBuf, Vec<PathBuf>>,
    ordered_dirs: Vec<PathBuf>,
    selected_dirs: Arc<HashSet<PathBuf>>,
    workspace_cycles: Option<Vec<Vec<PathBuf>>>,
    unmatched: Option<UnmatchedFilters>,
}

fn resolve_selection(
    projects: &[pnpm_workspace::Project],
    cfg: &Config,
    prefix: &Path,
    auto_exclude_root: AutoExcludeRoot<'_>,
    (recursive_sort, precompute_workspace_cycles): (bool, bool),
) -> miette::Result<ResolvedSelection> {
    let (selection, unmatched) =
        select_recursive_projects_deferring_no_match(projects, cfg, prefix, auto_exclude_root)?;
    let workspace_cycles =
        precomputed_workspace_cycles(&selection, cfg, precompute_workspace_cycles);
    let project_dependencies = project_dependencies(&selection, recursive_sort);
    Ok(ResolvedSelection {
        ordered_dirs: sequence_project_dependencies(&project_dependencies),
        selected_dirs: selected_project_dirs(&selection),
        project_dependencies,
        workspace_cycles,
        unmatched,
    })
}

fn selected_project_dirs(selection: &RecursiveSelection<'_>) -> Arc<HashSet<PathBuf>> {
    Arc::new(
        selection.selected
            .keys()
            .cloned()
            .collect(),
    )
}

/// The selected projects plus the workspace root project, when the
/// workspace root is a project.
fn install_dirs(
    selected_dirs: &HashSet<PathBuf>,
    projects: &[pnpm_workspace::Project],
    workspace_root: &Path,
) -> HashSet<PathBuf> {
    let normalized_workspace_root = pnpm_fs::lexical_normalize(workspace_root);
    let mut install_dirs = selected_dirs.clone();
    if let Some(workspace_root_project) = projects
        .iter()
        .find(|project| pnpm_fs::lexical_normalize(&project.root_dir) == normalized_workspace_root)
    {
        install_dirs.insert(workspace_root_project.root_dir.clone());
    }
    install_dirs
}
