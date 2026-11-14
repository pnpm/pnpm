mod discovery;
mod fs_ops;
mod removal;

use super::{
    dispatch::RunCtx, lockfile_dir::LockfileDirArg, pipelines::InstallFamilyPlan,
    recursive::selectors_narrow_the_run,
};
use discovery::{ci_clean_targets, select_clean_targets};
use pnpm_config::Config;
use pnpm_package_manager::FrozenVirtualStoreClean;
use removal::CleanRun;
use std::path::Path;

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

#[derive(Clone, Copy)]
pub(super) enum LockfileRemoval {
    Keep,
    Remove,
}

#[derive(Clone, Copy)]
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
) -> miette::Result<()> {
    let root_dir = config.workspace_dir.as_deref().unwrap_or(ctx.locations.dir);
    let Ok(real_root_dir) = dunce::canonicalize(root_dir) else {
        return Ok(());
    };
    let Some(targets) = select_clean_targets(ctx, config, root_dir, &real_root_dir)? else {
        return Ok(());
    };
    CleanRun {
        config,
        root_dir,
        real_root_dir: &real_root_dir,
        lockfile_removal,
        scope: targets.scope,
        raw_lockfile_dir: raw_configured_lockfile_dir(config),
        protected_member_dirs: &targets.protected_member_dirs,
    }
    .clean_dirs(&targets.dirs)
}

/// Clean the `node_modules` directories `pnpm ci` is about to reinstall,
/// after frozen-lockfile validation has succeeded.
pub(crate) fn run_for_ci(
    config: &Config,
    prefix: &Path,
    plan: &InstallFamilyPlan,
) -> miette::Result<()> {
    let root_dir = config.workspace_dir.as_deref().unwrap_or(prefix);
    let Ok(real_root_dir) = dunce::canonicalize(root_dir) else {
        return Ok(());
    };
    let scope = if is_narrowed_workspace_clean(config) {
        CleanScope::Narrowed
    } else {
        CleanScope::WholeWorkspace
    };
    let targets = ci_clean_targets(config, prefix, root_dir, &real_root_dir, plan, scope)?;
    CleanRun {
        config,
        root_dir,
        real_root_dir: &real_root_dir,
        lockfile_removal: LockfileRemoval::Keep,
        scope: targets.scope,
        raw_lockfile_dir: raw_configured_lockfile_dir(config),
        protected_member_dirs: &targets.protected_member_dirs,
    }
    .clean_dirs(&targets.dirs)
}

/// Whether [`run_for_ci`] removes or preserves `<install_state_dir>/lock.yaml`
/// for the install anchored at `prefix`.
pub(crate) fn ci_virtual_store_clean(config: &Config, prefix: &Path) -> FrozenVirtualStoreClean {
    let root_dir = config.workspace_dir.as_deref().unwrap_or(prefix);
    let Ok(real_root_dir) = dunce::canonicalize(root_dir) else {
        return FrozenVirtualStoreClean::PreservesVirtualStore;
    };
    let scope = if is_narrowed_workspace_clean(config) {
        CleanScope::Narrowed
    } else {
        CleanScope::WholeWorkspace
    };
    if matches!(scope, CleanScope::Narrowed)
        && config.workspace_dir.is_some()
        && config.shares_one_lockfile()
    {
        return FrozenVirtualStoreClean::PreservesVirtualStore;
    }
    let Some(real_state_dir) = fs_ops::resolve_inside(&real_root_dir, &config.install_state_dir)
    else {
        return FrozenVirtualStoreClean::PreservesVirtualStore;
    };
    if !config.shares_one_lockfile()
        && config.workspace_dir.is_some()
        && !dedicated_state_removed_by_clean(config, prefix, &real_root_dir, &real_state_dir, scope)
    {
        return FrozenVirtualStoreClean::PreservesVirtualStore;
    }
    FrozenVirtualStoreClean::RemovesVirtualStore
}

fn dedicated_state_removed_by_clean(
    config: &Config,
    project_dir: &Path,
    real_root_dir: &Path,
    real_state_dir: &Path,
    scope: CleanScope,
) -> bool {
    let root_dir = config.workspace_dir.as_deref().unwrap_or(project_dir);
    let in_project = dunce::canonicalize(project_dir)
        .ok()
        .and_then(|real_project| fs_ops::resolve_inside(&real_project, &config.install_state_dir))
        .is_some();
    let in_whole_workspace_vs = matches!(scope, CleanScope::WholeWorkspace)
        && fs_ops::resolve_inside(
            real_root_dir,
            &removal::configured_virtual_store_dir(root_dir, config),
        )
        .is_some_and(|vs_dir| pnpm_fs::is_subdir(&vs_dir, real_state_dir));
    in_project || in_whole_workspace_vs
}

pub(super) fn is_narrowed_workspace_clean(config: &Config) -> bool {
    selectors_narrow_the_run(config)
        || (config.workspace_dir.is_some()
            && !config.recursive
            && config.cli_settings.contains("recursive"))
}

fn raw_configured_lockfile_dir(config: &Config) -> Option<&Path> {
    config.configured_lockfile_dir.as_deref().or(config.lockfile_dir.as_deref())
}
