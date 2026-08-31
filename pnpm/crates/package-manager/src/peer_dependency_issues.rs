//! Install-time reporting of unmet peer dependencies.
//!
//! Mirrors pnpm's `reportPeerDependencyIssues`: an install that
//! resolved ends by applying `peerDependencyRules` to the issues its
//! resolution left behind, then either failing the install
//! (`strictPeerDependencies`) or logging them for the reporter. An
//! install that did not resolve — a frozen or up-to-date-lockfile run —
//! reports nothing, so `pnpm peers check` is what answers for a tree
//! nobody re-resolved.

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
/// `peer_issue_importer_ids` identifies importers for which the resolver found peer issues.
/// This function checks only those importers against the final lockfile.
///
/// Resolver parent chains omit package versions.
/// The final lockfile supplies those versions to the report.
/// `installed_importer_ids` prevents a filtered install from reporting
/// issues for unselected projects.
pub(crate) fn report_peer_dependency_issues<Reporter: pnpm_reporter::Reporter>(
    resolved_lockfile: Option<&Lockfile>,
    peer_issue_importer_ids: &HashSet<String>,
    installed_importer_ids: &HashSet<String>,
    lockfile_dir: &Path,
    config: &Config,
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
    if importer_ids.is_empty() {
        return Ok(());
    }
    importer_ids.sort();
    let Some(report) = peer_issues_for_lockfile(
        lockfile,
        lockfile_dir,
        &importer_ids,
        &config.peer_dependency_rules,
    ) else {
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
