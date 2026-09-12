//! Install-time reporting of unmet peer dependencies.
//!
//! Mirrors pnpm's `reportPeerDependencyIssues`: an install that
//! resolved ends by applying `peerDependencyRules` to the issues its
//! resolution left behind, then either failing the install
//! (`strictPeerDependencies`) or logging them for the reporter. An
//! install that did not resolve — a frozen or up-to-date-lockfile run —
//! reports nothing, so `pnpm peers check` is what answers for a tree
//! nobody re-resolved.

use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_deps_inspection_peers::peer_issues_for_lockfile;
use pnpm_lockfile::Lockfile;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, PeerDependencyIssuesLog};
use std::{collections::HashSet, path::Path};

use crate::InstallError;

/// Report the peer-dependency issues of the lockfile an install just
/// resolved. `None` — every install that skipped resolution — reports
/// nothing.
///
/// `peer_issue_importer_ids` is the resolution's own verdict: the
/// importers it left an issue under. Only those are walked, so a
/// workspace the resolution found clean costs nothing here. The
/// lockfile stays the report's source — it carries the resolved
/// versions the resolver's parent chains leave out.
///
/// `installed_importer_ids` scopes the verdict to the projects this run
/// acted on. A `--filter`ed install leaves every unselected importer in
/// the lockfile untouched, and pnpm reports only on the projects that
/// took part in the resolution.
///
/// `catalogs` is `None` when the install has no catalog context. Raw
/// `catalog:` peer ranges in externally linked packages then remain peer
/// issues instead of becoming catalog-configuration errors.
pub(crate) fn report_peer_dependency_issues<Reporter: pnpm_reporter::Reporter>(
    resolved_lockfile: Option<&Lockfile>,
    peer_issue_importer_ids: &HashSet<String>,
    installed_importer_ids: &HashSet<String>,
    lockfile_dir: &Path,
    config: &Config,
    catalogs: Option<&Catalogs>,
) -> Result<(), InstallError> {
    let Some(lockfile) = resolved_lockfile else { return Ok(()) };
    let mut importer_ids: Vec<String> = peer_issue_importer_ids
        .iter()
        .filter(|importer_id| {
            installed_importer_ids.contains(*importer_id)
                && lockfile.importers.contains_key(*importer_id)
        })
        .cloned()
        .collect();
    importer_ids.sort();
    let Some(report) = peer_issues_for_lockfile(
        lockfile,
        lockfile_dir,
        &importer_ids,
        &config.peer_dependency_rules,
        catalogs,
    )
    .map_err(InstallError::CatalogResolution)?
    else {
        return Ok(());
    };
    if config.strict_peer_dependencies {
        // The listing and its hints go out through the reporter, in
        // pnpm's own error format; `is_reported_error` then keeps the
        // CLI from rendering the returned error a second time.
        Reporter::emit(&LogEvent::Global(GlobalLog {
            level: LogLevel::Error,
            message: report.render_error(),
        }));
        return Err(InstallError::PeerDependencyIssues);
    }
    Reporter::emit(&LogEvent::PeerDependencyIssues(PeerDependencyIssuesLog {
        level: LogLevel::Debug,
        issues_by_projects: serde_json::to_value(report.issues()).unwrap_or_default(),
    }));
    Ok(())
}

#[cfg(test)]
mod tests;
