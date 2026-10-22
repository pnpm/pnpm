//! `syncInjectedDepsAfterScripts` around one script: publish injected
//! copies while it runs, then hardlink-sync them after it succeeds.

use super::{RunContext, Value};
use pnpm_config::Config;
use pnpm_injected_deps_syncer::{
    BinLinkSettings, InjectedEditWatch, ProjectModulesDir, SyncInjectedDeps, injected_edit_sources,
    sync_injected_deps, watch_injected_edits,
};
use pnpm_workspace::{FindWorkspaceProjectsOpts, find_workspace_projects};

pub(super) fn start_injected_edit_watch(
    ctx: &RunContext<'_>,
    name: &str,
) -> Option<InjectedEditWatch> {
    if !syncs_injected_deps_after(ctx, name) {
        return None;
    }
    // The sync after the script reports a failure to list the projects, so
    // it does not stop the script from starting.
    let project_modules_dirs = project_modules_dirs(ctx.config).unwrap_or_default();
    let sources = injected_edit_sources(&injected_sync_opts(ctx, &project_modules_dirs));
    (!sources.is_empty()).then(|| watch_injected_edits(sources))
}

pub(super) fn sync_injected_deps_after(ctx: &RunContext<'_>, name: &str) -> miette::Result<()> {
    if syncs_injected_deps_after(ctx, name) {
        let project_modules_dirs = project_modules_dirs(ctx.config)?;
        sync_injected_deps(&injected_sync_opts(ctx, &project_modules_dirs))?;
    }
    Ok(())
}

/// See [`SyncInjectedDeps::project_modules_dirs`]. Each project's modules
/// directory is the one its install uses.
pub(crate) fn project_modules_dirs(config: &Config) -> miette::Result<Vec<ProjectModulesDir>> {
    let Some(workspace_dir) = config.workspace_dir.as_deref() else { return Ok(Vec::new()) };
    if config.shared_workspace_lockfile {
        return Ok(Vec::new());
    }
    let projects = find_workspace_projects(
        workspace_dir,
        &FindWorkspaceProjectsOpts {
            patterns: config.workspace_package_patterns.clone(),
            ignored_directories: config.managed_directories(),
        },
    )
    .map_err(miette::Report::new)?;
    Ok(projects
        .into_iter()
        .map(|project| {
            let name = project.manifest
                .value()
                .get("name")
                .and_then(Value::as_str);
            ProjectModulesDir {
                modules_dir: config.project_modules_dir(&project.root_dir, name),
                project_dir: project.root_dir,
            }
        })
        .collect())
}

fn syncs_injected_deps_after(ctx: &RunContext<'_>, name: &str) -> bool {
    ctx.config.sync_injected_deps_after_scripts
        .iter()
        .any(|script| script == name)
}

fn injected_sync_opts<'a>(
    ctx: &'a RunContext<'_>,
    project_modules_dirs: &'a [ProjectModulesDir],
) -> SyncInjectedDeps<'a> {
    SyncInjectedDeps {
        pkg_name: ctx.manifest
            .value()
            .get("name")
            .and_then(Value::as_str),
        pkg_root_dir: ctx.dir,
        workspace_dir: ctx.config.workspace_dir.as_deref(),
        workspace_modules_dir: &ctx.config.modules_dir,
        include_only_package_files: !ctx.config.deploy_all_files,
        // Read before the script ran, so a bin it drops can still be named.
        manifest_before_scripts: Some(ctx.manifest.value()),
        project_modules_dirs,
        bin_links: BinLinkSettings {
            modules_dir_name: ctx.config.modules_dir_name(),
            extend_node_path: ctx.config.extend_node_path,
            ignored_directories: ctx.config.managed_directories(),
        },
    }
}
