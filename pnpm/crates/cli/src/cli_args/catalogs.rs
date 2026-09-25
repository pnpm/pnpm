//! The catalogs a command dereferences `catalog:` specifiers against.

use miette::{Context, IntoDiagnostic};
use pnpm_catalogs_config::get_catalogs_from_workspace_manifest;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_workspace::read_workspace_manifest;
use pnpm_workspace_projects_graph::WorkspaceCatalogs;

/// The hook-injected set when an `updateConfig` pnpmfile provided one
/// ([`Config::catalogs`] is `Some`), otherwise the `catalog:` /
/// `catalogs:` tables of the workspace manifest — the same fallback the
/// install performs. Empty outside a workspace, where no catalog can be
/// declared.
pub(crate) fn configured_catalogs(config: &Config) -> miette::Result<Catalogs> {
    if let Some(catalogs) = &config.catalogs {
        return Ok(catalogs.clone());
    }
    let Some(workspace_dir) = config.workspace_dir.as_deref() else {
        return Ok(Catalogs::default());
    };
    let workspace_manifest = read_workspace_manifest(workspace_dir)
        .into_diagnostic()
        .wrap_err("read the workspace manifest for catalogs")?;
    get_catalogs_from_workspace_manifest(workspace_manifest.as_ref())
        .into_diagnostic()
        .wrap_err("read the workspace catalogs")
}

/// `catalogs` paired with the workspace directory their relative paths
/// are measured from, for building the workspace project graph. `None`
/// outside a workspace.
pub(crate) fn workspace_catalogs<'a>(
    config: &'a Config,
    catalogs: &'a Catalogs,
) -> Option<WorkspaceCatalogs<'a>> {
    config.workspace_dir
        .as_deref()
        .map(|workspace_dir| WorkspaceCatalogs { catalogs, workspace_dir })
}
