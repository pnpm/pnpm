use super::{
    super::{
        dispatch::RunCtx,
        pipelines::InstallFamilyPlan,
        recursive::{
            AutoExcludeRoot, discover_workspace_projects, notice_workspace_dir,
            print_no_projects_matched, select_recursive_projects,
        },
    },
    CleanScope, is_narrowed_workspace_clean,
    removal::{configured_modules_leaf, modules_leaf_for_project},
};
use miette::Context;
use pnpm_config::Config;
use pnpm_workspace::{FindWorkspaceProjectsOpts, Project, find_workspace_projects};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

pub(super) struct CleanTargets {
    pub(super) dirs: Vec<PathBuf>,
    pub(super) protected_member_dirs: Vec<PathBuf>,
    pub(super) scope: CleanScope,
}

pub(super) fn select_clean_targets(
    ctx: &RunCtx<'_>,
    config: &Config,
    root_dir: &Path,
    real_root_dir: &Path,
) -> miette::Result<Option<CleanTargets>> {
    if !config.filter.is_empty()
        || !config.filter_prod.is_empty()
        || (config.workspace_root && config.recursive)
    {
        return select_filtered_clean_targets(ctx, config, real_root_dir);
    }
    if is_narrowed_workspace_clean(config) {
        let single_dir = if config.workspace_root { root_dir } else { ctx.locations.dir };
        let protected_member_dirs = protected_members_if_needed(config, real_root_dir)?;
        return Ok(Some(CleanTargets {
            dirs: vec![single_dir.to_path_buf()],
            protected_member_dirs,
            scope: CleanScope::Narrowed,
        }));
    }
    if let Some(workspace_dir) = config.workspace_dir.as_deref() {
        let projects = discover_workspace_for_clean(workspace_dir, real_root_dir, config)?;
        let protected_member_dirs = collect_non_root_real_dirs(&projects, real_root_dir);
        let dirs = workspace_dirs_with_root(
            workspace_dir,
            real_root_dir,
            projects.into_iter().map(|project| project.root_dir),
        );
        return Ok(Some(CleanTargets {
            dirs,
            protected_member_dirs,
            scope: CleanScope::WholeWorkspace,
        }));
    }
    Ok(Some(CleanTargets {
        dirs: vec![ctx.locations.dir.to_path_buf()],
        protected_member_dirs: Vec::new(),
        scope: CleanScope::WholeWorkspace,
    }))
}

fn select_filtered_clean_targets(
    ctx: &RunCtx<'_>,
    config: &Config,
    real_root_dir: &Path,
) -> miette::Result<Option<CleanTargets>> {
    let projects = if let Some(workspace_dir) = config.workspace_dir.as_deref() {
        discover_workspace_for_clean(workspace_dir, real_root_dir, config)?
    } else {
        find_workspace_projects(
            ctx.locations.dir,
            &FindWorkspaceProjectsOpts {
                patterns: Some(vec![".".to_string()]),
                ignored_directories: config.managed_directories(),
            },
        )
        .wrap_err("finding workspace projects")?
    };
    let protected_member_dirs = if config.workspace_dir.is_some() {
        collect_non_root_real_dirs(&projects, real_root_dir)
    } else {
        Vec::new()
    };
    let selection =
        select_recursive_projects(&projects, config, ctx.locations.dir, AutoExcludeRoot::Disabled)?;
    if selection.selected.is_empty() {
        if projects.is_empty() {
            pnpm_diagnostics::println!(
                r#"No projects found in "{}""#,
                notice_workspace_dir(config, ctx.locations.dir).display(),
            );
        } else {
            print_no_projects_matched(config, ctx.locations.dir);
        }
        return Ok(None);
    }
    Ok(Some(CleanTargets {
        dirs: selection.selected
            .keys()
            .cloned()
            .collect(),
        protected_member_dirs,
        scope: CleanScope::Narrowed,
    }))
}

pub(super) fn ci_clean_targets(
    config: &Config,
    prefix: &Path,
    root_dir: &Path,
    real_root_dir: &Path,
    plan: &InstallFamilyPlan,
    scope: CleanScope,
) -> miette::Result<CleanTargets> {
    let protected_member_dirs = protected_members_if_needed(config, real_root_dir)?;
    let dirs = match scope {
        CleanScope::Narrowed => ci_narrowed_dirs(prefix, plan),
        CleanScope::WholeWorkspace => {
            ci_whole_workspace_dirs(config, prefix, root_dir, real_root_dir, plan)?
        }
    };
    Ok(CleanTargets { dirs, protected_member_dirs, scope })
}

fn ci_narrowed_dirs(prefix: &Path, plan: &InstallFamilyPlan) -> Vec<PathBuf> {
    match plan {
        InstallFamilyPlan::Shared(selection) => selection.ordered_dirs
            .iter()
            .filter(|dir| selection.selected_dirs.contains(*dir))
            .cloned()
            .collect(),
        InstallFamilyPlan::PerProject(projects) => projects.dependencies
            .keys()
            .cloned()
            .collect(),
        InstallFamilyPlan::Single => vec![prefix.to_path_buf()],
    }
}

fn ci_whole_workspace_dirs(
    config: &Config,
    prefix: &Path,
    root_dir: &Path,
    real_root_dir: &Path,
    plan: &InstallFamilyPlan,
) -> miette::Result<Vec<PathBuf>> {
    match plan {
        InstallFamilyPlan::Shared(selection) => Ok(workspace_dirs_with_root(
            &selection.workspace_root,
            real_root_dir,
            selection.projects.iter().map(|project| project.root_dir.clone()),
        )),
        InstallFamilyPlan::PerProject(projects) => Ok(workspace_dirs_with_root(
            root_dir,
            real_root_dir,
            projects.workspace
                .projects()
                .iter()
                .map(|project| project.root_dir.clone()),
        )),
        InstallFamilyPlan::Single => {
            let Some(workspace_dir) = config.workspace_dir.as_deref() else {
                return Ok(vec![prefix.to_path_buf()]);
            };
            let projects = discover_workspace_for_clean(workspace_dir, real_root_dir, config)?;
            Ok(workspace_dirs_with_root(
                workspace_dir,
                real_root_dir,
                projects.into_iter().map(|project| project.root_dir),
            ))
        }
    }
}

fn protected_members_if_needed(
    config: &Config,
    real_root_dir: &Path,
) -> miette::Result<Vec<PathBuf>> {
    let Some(workspace_dir) = config.workspace_dir.as_deref() else {
        return Ok(Vec::new());
    };
    if !has_custom_modules_dirs(workspace_dir, config) {
        return Ok(Vec::new());
    }
    let projects = discover_workspace_for_clean(workspace_dir, real_root_dir, config)?;
    Ok(collect_non_root_real_dirs(&projects, real_root_dir))
}

fn discover_workspace_for_clean(
    workspace_dir: &Path,
    real_root_dir: &Path,
    config: &Config,
) -> miette::Result<Vec<Project>> {
    let (mut projects, patterns) = discover_workspace_projects(workspace_dir, config)?;
    let includes_root = projects
        .iter()
        .any(|project| {
            dunce::canonicalize(&project.root_dir).ok().as_deref() == Some(real_root_dir)
        });
    if includes_root || config.modules_dir == workspace_dir.join("node_modules") {
        return Ok(projects);
    }
    let ignored_directories = config
        .managed_directories()
        .into_iter()
        .filter(|dir| dir != &config.modules_dir && dir != &config.install_state_dir)
        .collect();
    let unowned_candidates = find_workspace_projects(
        workspace_dir,
        &FindWorkspaceProjectsOpts { patterns, ignored_directories },
    )
    .wrap_err("finding workspace projects")?;
    merge_projects_outside_owned_modules(&mut projects, unowned_candidates, config);
    Ok(projects)
}

fn merge_projects_outside_owned_modules(
    known: &mut Vec<Project>,
    found: Vec<Project>,
    config: &Config,
) {
    let default_leaf = configured_modules_leaf(config);
    let known_dirs: HashSet<PathBuf> = known
        .iter()
        .map(|project| project.root_dir.clone())
        .collect();
    let mut owned_modules_dirs = HashMap::new();
    for project in known.iter().chain(found.iter()) {
        let leaf = modules_leaf_for_project(config, &project.root_dir, &default_leaf);
        if !leaf.is_absolute() {
            owned_modules_dirs.insert(project.root_dir.clone(), project.root_dir.join(leaf));
        }
    }
    for candidate in found {
        if !known_dirs.contains(&candidate.root_dir)
            && !is_inside_owned_modules_dir(&candidate.root_dir, &owned_modules_dirs)
        {
            known.push(candidate);
        }
    }
}

fn is_inside_owned_modules_dir(dir: &Path, owned_modules_dirs: &HashMap<PathBuf, PathBuf>) -> bool {
    let mut current = dir.parent();
    while let Some(ancestor) = current {
        if owned_modules_dirs
            .get(ancestor)
            .is_some_and(|modules_dir| dir.starts_with(modules_dir) && dir != modules_dir)
        {
            return true;
        }
        current = ancestor.parent();
    }
    false
}

fn has_custom_modules_dirs(workspace_dir: &Path, config: &Config) -> bool {
    config.modules_dir != workspace_dir.join("node_modules")
        || config.package_configs
            .as_ref()
            .is_some_and(|configs| {
                configs.values().any(|entry| entry.modules_dir.is_some())
            })
}

fn collect_non_root_real_dirs(projects: &[Project], real_root_dir: &Path) -> Vec<PathBuf> {
    projects
        .iter()
        .filter_map(|project| dunce::canonicalize(&project.root_dir).ok())
        .filter(|dir| dir != real_root_dir)
        .collect()
}

fn workspace_dirs_with_root(
    workspace_dir: &Path,
    real_root_dir: &Path,
    project_dirs: impl IntoIterator<Item = PathBuf>,
) -> Vec<PathBuf> {
    let project_dirs: Vec<PathBuf> = project_dirs.into_iter().collect();
    let includes_root = project_dirs
        .iter()
        .any(|dir| dunce::canonicalize(dir).ok().as_deref() == Some(real_root_dir));
    if includes_root {
        project_dirs
    } else {
        std::iter::once(workspace_dir.to_path_buf()).chain(project_dirs).collect()
    }
}
