//! `syncInjectedDepsAfterScripts` around one script: publish injected
//! copies while it runs, then hardlink-sync them after it succeeds.

use super::{RunContext, Value};
use pnpm_injected_deps_syncer::{
    InjectedEditWatch, SyncInjectedDeps, injected_edit_sources, sync_injected_deps,
    watch_injected_edits,
};

pub(super) fn start_injected_edit_watch(
    ctx: &RunContext<'_>,
    name: &str,
) -> Option<InjectedEditWatch> {
    if !syncs_injected_deps_after(ctx, name) {
        return None;
    }
    let sources = injected_edit_sources(&injected_sync_opts(ctx));
    (!sources.is_empty()).then(|| watch_injected_edits(sources))
}

pub(super) fn sync_injected_deps_after(ctx: &RunContext<'_>, name: &str) -> miette::Result<()> {
    if syncs_injected_deps_after(ctx, name) {
        sync_injected_deps(&injected_sync_opts(ctx))?;
    }
    Ok(())
}

fn syncs_injected_deps_after(ctx: &RunContext<'_>, name: &str) -> bool {
    ctx.config.sync_injected_deps_after_scripts
        .iter()
        .any(|script| script == name)
}

fn injected_sync_opts<'a>(ctx: &'a RunContext<'_>) -> SyncInjectedDeps<'a> {
    SyncInjectedDeps {
        pkg_name: ctx.manifest
            .value()
            .get("name")
            .and_then(Value::as_str),
        pkg_root_dir: ctx.dir,
        workspace_dir: ctx.config.workspace_dir.as_deref(),
        modules_dir_name: ctx.config.modules_dir_name(),
        workspace_modules_dir: &ctx.config.modules_dir,
        extend_node_path: ctx.config.extend_node_path,
        // Read before the script ran, so a bin it drops can still be named.
        manifest_before_scripts: Some(ctx.manifest.value()),
        ignored_directories: ctx.config.managed_directories(),
    }
}
