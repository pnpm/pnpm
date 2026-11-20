//! The fast path's check on the manifest files, which discovery reads.

use super::{
    Config,
    CoveredProjects,
    FastPathContext,
    FastPathProjects,
    UpToDateFastPathCheck,
    WorkspaceState,
    build_project_manifests_list,
    check_fast_path_repeat_install,
};

/// The repeat-install check on the manifests discovery reads. Discovery
/// reads the projects `listed` names, when the caller already listed them.
pub(super) fn check_from_files(
    check: &UpToDateFastPathCheck<'_>,
    context: &FastPathContext<'_>,
    state: &WorkspaceState,
    listed: Option<Vec<pnpm_workspace::ProjectManifestPaths>>,
) -> Result<CoveredProjects, &'static str> {
    let workspace_projects = discover_fast_path_projects(check.config, context, listed)?;
    let project_manifests =
        build_project_manifests_list(check.manifest, workspace_projects.as_deref());
    check_fast_path_repeat_install(
        check,
        context,
        FastPathProjects {
            state,
            manifests: &project_manifests,
            recorded_digests: None,
            bin_projects: None,
        },
    )?;
    Ok(CoveredProjects::of(
        workspace_projects.as_ref().map(Vec::len),
        &project_manifests,
        check.config.runtime_on_fail.map(pnpm_config::RuntimeOnFail::as_str),
    ))
}

/// [`crate::install::read_checked_workspace_projects`] inside the
/// `discover_workspace_projects` span.
fn discover_fast_path_projects(
    config: &Config,
    context: &FastPathContext<'_>,
    listed: Option<Vec<pnpm_workspace::ProjectManifestPaths>>,
) -> Result<Option<Vec<pnpm_workspace::Project>>, &'static str> {
    let _span =
        tracing::debug_span!(target: "pacquet::install", "discover_workspace_projects").entered();
    crate::install::read_checked_workspace_projects(
        config,
        (context.workspace_root, context.workspace_manifest),
        listed,
    )
    .map_err(|_| "the workspace projects cannot be listed")
}
