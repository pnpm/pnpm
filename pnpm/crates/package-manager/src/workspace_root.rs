use pnpm_config::Config;
use std::path::{
    Path,
    PathBuf,
};

/// The workspace root the run belongs to: the configured one, else the
/// nearest ancestor carrying a `pnpm-workspace.yaml`.
///
/// Yields `None` under [`Config::workspace_search_skipped`], where the
/// ancestor walk would re-adopt the very `pnpm-workspace.yaml` that
/// `--ignore-workspace` asked to ignore. Only that walk is suppressed:
/// a caller that pinned [`Config::workspace_dir`] keeps it, which is how a
/// global install anchors itself under the global packages dir.
pub(crate) fn configured_or_discovered_workspace_dir(
    config: &Config,
    manifest_dir: &Path,
) -> Result<Option<PathBuf>, pnpm_workspace::FindWorkspaceDirError> {
    match &config.workspace_dir {
        Some(workspace_dir) => Ok(Some(workspace_dir.clone())),
        None if config.workspace_search_skipped => Ok(None),
        None => pnpm_workspace::find_workspace_dir(manifest_dir),
    }
}
