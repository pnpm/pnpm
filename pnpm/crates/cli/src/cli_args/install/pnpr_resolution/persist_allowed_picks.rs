use crate::State;

pub(super) fn persist_allowed_picks(
    state: &State,
    lockfile_dir: &std::path::Path,
    link: &super::PnprLink<'_>,
    allowed_picks: &[String],
) -> miette::Result<()> {
    if !link.save_workspace_manifest || allowed_picks.is_empty() {
        return Ok(());
    }
    let workspace_dir = state.config.target_workspace_dir.as_deref().unwrap_or(lockfile_dir);
    pnpm_workspace_manifest_writer::update_workspace_manifest(
        workspace_dir,
        &pnpm_workspace_manifest_writer::UpdateWorkspaceManifestOptions {
            added_minimum_release_age_excludes: allowed_picks,
            ..Default::default()
        },
    )
    .map_err(|err| miette::Report::new(err).wrap_err("persist minimumReleaseAgeExclude"))
}
