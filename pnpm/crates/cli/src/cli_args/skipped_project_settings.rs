//! A project's `pnpm-workspace.yaml` does not decide where pnpm keeps its
//! credentials, its own installation, or the directories a command works in.
//! `WorkspaceSettings` declares none of those keys, so they are dropped at
//! parse time — this says which ones a manifest carried, so the setting isn't
//! silently ignored.

use super::config_warnings::emit_config_warning;
use itertools::Itertools;
use pacquet_config::Config;

/// Warn about every skipped setting the project's `pnpm-workspace.yaml`
/// declared, as its own load noticed them. This is a config-load warning, so
/// it goes to stderr through [`emit_config_warning`] rather than the reporter.
pub(crate) fn warn_skipped_project_settings(config: &Config) {
    let ignored = &config.skipped_project_settings;
    if ignored.is_empty() {
        return;
    }
    let keys = ignored.iter().format_with(", ", |key, fmt| fmt(&format_args!("{key:?}")));
    emit_config_warning(&format!(
        "The following settings cannot be set in a project's pnpm-workspace.yaml \
         and were ignored: {keys}.",
    ));
}
