use pnpm_config::Config;
use pnpm_injected_deps_syncer::{SyncInjectedDeps, WorkspaceModules, sync_injected_deps};
use serde_json::Value;
use std::path::Path;

/// Bring the injected copies of `pkg_root_dir` back in step after one of its
/// scripts ran, so a bin the script added, dropped, or rewrote reaches them.
///
/// `manifest` is the project's `package.json` as it was *before* the script: a
/// bin the script dropped no longer declares itself there, and the injected
/// copies' own `package.json` is hardlinked to the source, so the in-place
/// rewrite has already reached them.
pub(crate) fn sync_project_injected_deps(
    config: &Config,
    pkg_root_dir: &Path,
    manifest: &Value,
) -> Result<(), pnpm_injected_deps_syncer::SyncInjectedDepsError> {
    sync_injected_deps(&SyncInjectedDeps {
        pkg_name: manifest.get("name").and_then(Value::as_str),
        pkg_root_dir,
        workspace_dir: config.workspace_dir.as_deref(),
        workspace_modules: WorkspaceModules {
            modules_dir_name: config.modules_dir_name(),
            dir: &config.modules_dir,
            extend_node_path: config.extend_node_path,
        },
        manifest_before_scripts: Some(manifest),
        ignored_directories: config.managed_directories(),
        link_options: pnpm_deps_restorer::shim_link_options(config, config.node_linker),
    })
}
