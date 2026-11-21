use crate::State;
use pnpm_package_manager::persist_loose_release_age_picks;
use pnpm_reporter::Reporter;

pub(super) fn persist_allowed_picks<ReporterImpl: Reporter>(
    state: &State,
    lockfile_dir: &std::path::Path,
    link: &super::PnprLink<'_>,
    allowed_picks: &[String],
) -> miette::Result<()> {
    if !link.save_workspace_manifest || allowed_picks.is_empty() {
        return Ok(());
    }
    let workspace_dir = state.config.target_workspace_dir.as_deref().unwrap_or(lockfile_dir);
    persist_loose_release_age_picks::<ReporterImpl>(workspace_dir, allowed_picks)
        .map_err(miette::Report::new)
}
