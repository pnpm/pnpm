//! A project's `pnpm-workspace.yaml` does not decide where pnpm keeps its
//! credentials, its own installation, or the directories a command works in.
//! `WorkspaceSettings` declares none of those keys, so they are dropped at
//! parse time — this says which ones a manifest carried, so the setting isn't
//! silently ignored.

use super::config_warnings::emit_config_warning;
use pacquet_config::skipped_project_settings;
use std::path::Path;

/// Warn about every skipped setting the project's `pnpm-workspace.yaml`
/// declares. This is a config-load warning, so it goes to stderr through
/// [`emit_config_warning`] rather than the reporter.
pub(crate) fn warn_skipped_project_settings(workspace_dir: &Path) {
    let ignored = skipped_project_settings(workspace_dir);
    if ignored.is_empty() {
        return;
    }
    let keys = ignored.iter().map(|key| format!("{key:?}")).collect::<Vec<_>>().join(", ");
    emit_config_warning(&format!(
        "The following settings cannot be set in a project's pnpm-workspace.yaml \
         and were ignored: {keys}.",
    ));
}
