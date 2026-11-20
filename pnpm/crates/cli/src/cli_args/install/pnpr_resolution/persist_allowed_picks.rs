use crate::State;
use pnpm_reporter::{
    LogEvent,
    LogLevel,
    PnpmLog,
    Reporter,
};

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
    pnpm_workspace_manifest_writer::update_workspace_manifest(
        workspace_dir,
        &pnpm_workspace_manifest_writer::UpdateWorkspaceManifestOptions {
            added_minimum_release_age_excludes: allowed_picks,
            ..Default::default()
        },
    )
    .map_err(miette::Report::new)
    .map_err(|err| err.wrap_err("persist minimumReleaseAgeExclude"))?;

    let count = allowed_picks.len();
    let entry_word = if count == 1 { "entry" } else { "entries" };
    ReporterImpl::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Info,
        message: format!(
            "Added {count} {entry_word} to minimumReleaseAgeExclude in pnpm-workspace.yaml (set minimumReleaseAgeStrict to true to gate these updates with a prompt):\n  {}",
            allowed_picks.join("\n  "),
        ),
        prefix: workspace_dir.to_string_lossy().into_owned(),
    }));
    Ok(())
}
