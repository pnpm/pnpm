mod removal;

use super::{
    dispatch::RunCtx,
    lockfile_dir::LockfileDirArg,
    pipelines::InstallFamilyPlan,
    recursive::{
        AutoExcludeRoot, discover_workspace_projects, notice_workspace_dir,
        print_no_projects_matched, select_recursive_projects, selectors_narrow_the_run,
    },
};
use miette::Context;
use pnpm_config::Config;
use pnpm_fs::to_native_separators;
use pnpm_workspace::{FindWorkspaceProjectsOpts, find_workspace_projects};
use removal::CleanRun;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// `pnpm clean` / `pnpm purge`: safely remove the `node_modules`
/// directories of the current project (or the selected workspace projects)
/// without following NTFS junctions into their targets. A `clean` /
/// `purge` script in `package.json` overrides the built-in command;
/// `pnpm pm clean` / `pnpm pm purge` runs the built-in regardless.
#[derive(Debug, clap::Args)]
pub struct CleanArgs {
    /// Also remove `pnpm-lock.yaml` files.
    #[clap(short = 'l', long = "lockfile")]
    pub lockfile: bool,
    #[clap(flatten)]
    pub lockfile_dir: LockfileDirArg,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LockfileRemoval {
    Keep,
    Remove,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CleanScope {
    WholeWorkspace,
    Narrowed,
}

/// Remove `node_modules` contents and (optionally) lockfiles from the
/// current project or the selected workspace projects.
pub(super) fn run(
    ctx: &RunCtx<'_>,
    config: &Config,
    lockfile_removal: LockfileRemoval,
    cli_lockfile_dir: Option<&Path>,
) -> miette::Result<()> {
    let root_dir = config.workspace_dir.as_deref().unwrap_or(ctx.locations.dir);
    let Ok(real_root_dir) = dunce::canonicalize(root_dir) else {
        return Ok(());
    };
    let Some((dirs, scope)) = select_clean_targets(ctx, config, root_dir, &real_root_dir)? else {
        return Ok(());
    };
    let raw_lockfile_dir =
        raw_configured_lockfile_dir(config, ctx.locations.dir, root_dir, cli_lockfile_dir);
    CleanRun {
        config,
        root_dir,
        real_root_dir: &real_root_dir,
        lockfile_removal,
        scope,
        raw_lockfile_dir: raw_lockfile_dir.as_deref(),
    }
    .clean_dirs(&dirs)
}

/// Clean the `node_modules` directories `pnpm ci` is about to reinstall,
/// after frozen-lockfile validation has succeeded.
pub(crate) fn run_for_ci(
    config: &Config,
    prefix: &Path,
    plan: &InstallFamilyPlan,
    cli_lockfile_dir: Option<&Path>,
) -> miette::Result<()> {
    let root_dir = config.workspace_dir.as_deref().unwrap_or(prefix);
    let Ok(real_root_dir) = dunce::canonicalize(root_dir) else {
        return Ok(());
    };
    let scope = if selectors_narrow_the_run(config) {
        CleanScope::Narrowed
    } else {
        CleanScope::WholeWorkspace
    };
    let dirs = ci_clean_dirs(config, prefix, root_dir, &real_root_dir, plan, scope)?;
    let raw_lockfile_dir = raw_configured_lockfile_dir(config, prefix, root_dir, cli_lockfile_dir);
    CleanRun {
        config,
        root_dir,
        real_root_dir: &real_root_dir,
        lockfile_removal: LockfileRemoval::Keep,
        scope,
        raw_lockfile_dir: raw_lockfile_dir.as_deref(),
    }
    .clean_dirs(&dirs)
}

fn select_clean_targets(
    ctx: &RunCtx<'_>,
    config: &Config,
    root_dir: &Path,
    real_root_dir: &Path,
) -> miette::Result<Option<(Vec<PathBuf>, CleanScope)>> {
    if !config.filter.is_empty()
        || !config.filter_prod.is_empty()
        || (config.workspace_root && config.recursive)
    {
        return select_narrowed_clean_targets(ctx, config);
    }
    if config.workspace_root {
        return Ok(Some((vec![root_dir.to_path_buf()], CleanScope::Narrowed)));
    }
    if let Some(workspace_dir) = config.workspace_dir.as_deref() {
        let (projects, _patterns) = discover_workspace_projects(workspace_dir, config)?;
        let dirs = workspace_dirs_with_root(
            workspace_dir,
            real_root_dir,
            projects.into_iter().map(|project| project.root_dir),
        );
        return Ok(Some((dirs, CleanScope::WholeWorkspace)));
    }
    Ok(Some((vec![ctx.locations.dir.to_path_buf()], CleanScope::WholeWorkspace)))
}

fn select_narrowed_clean_targets(
    ctx: &RunCtx<'_>,
    config: &Config,
) -> miette::Result<Option<(Vec<PathBuf>, CleanScope)>> {
    let projects = if let Some(workspace_dir) = config.workspace_dir.as_deref() {
        discover_workspace_projects(workspace_dir, config)?.0
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
    Ok(Some((
        selection.selected
            .keys()
            .cloned()
            .collect(),
        CleanScope::Narrowed,
    )))
}

fn ci_clean_dirs(
    config: &Config,
    prefix: &Path,
    root_dir: &Path,
    real_root_dir: &Path,
    plan: &InstallFamilyPlan,
    scope: CleanScope,
) -> miette::Result<Vec<PathBuf>> {
    match scope {
        CleanScope::Narrowed => Ok(ci_narrowed_dirs(prefix, plan)),
        CleanScope::WholeWorkspace => {
            ci_whole_workspace_dirs(config, prefix, root_dir, real_root_dir, plan)
        }
    }
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
            let (projects, _) = discover_workspace_projects(workspace_dir, config)?;
            Ok(workspace_dirs_with_root(
                workspace_dir,
                real_root_dir,
                projects.into_iter().map(|project| project.root_dir),
            ))
        }
    }
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

fn raw_configured_lockfile_dir(
    config: &Config,
    cli_dir: &Path,
    root_dir: &Path,
    cli_lockfile_dir: Option<&Path>,
) -> Option<PathBuf> {
    let candidate = raw_lockfile_dir_candidate(config, cli_dir, root_dir, cli_lockfile_dir);
    match (&candidate, &config.lockfile_dir) {
        (Some(raw), Some(resolved)) if &pnpm_fs::lexical_normalize(raw) == resolved => candidate,
        _ => config.lockfile_dir.clone(),
    }
}

fn raw_lockfile_dir_candidate(
    config: &Config,
    cli_dir: &Path,
    root_dir: &Path,
    cli_lockfile_dir: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(cli_lockfile_dir) = cli_lockfile_dir {
        return Some(anchor_raw_path(cli_lockfile_dir, cli_dir));
    }
    let explicit = config.explicit_settings.get("lockfileDir").and_then(Value::as_str);
    if config.cli_settings.contains("lockfileDir")
        && let Some(raw) = explicit
    {
        return Some(anchor_raw_path(Path::new(raw), cli_dir));
    }
    if let Some(env_raw) = env_lockfile_dir() {
        return Some(anchor_raw_path(Path::new(&env_raw), cli_dir));
    }
    explicit.map(|raw| anchor_raw_path(Path::new(raw), root_dir))
}

fn env_lockfile_dir() -> Option<String> {
    std::env::var("PNPM_CONFIG_LOCKFILE_DIR")
        .or_else(|_| std::env::var("pnpm_config_lockfile_dir"))
        .ok()
        .filter(|value| !value.is_empty())
}

fn anchor_raw_path(raw: &Path, anchor: &Path) -> PathBuf {
    let raw_path = to_native_separators(raw);
    if raw_path.is_absolute() { raw_path.into_owned() } else { anchor.join(raw_path) }
}
