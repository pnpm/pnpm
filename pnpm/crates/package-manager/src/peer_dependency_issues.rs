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
use pnpm_deps_inspection_peers::{
    IssuesByProjects, check_all_importers, filter_peer_issues, has_missing_peers,
    has_reportable_issues, render_peer_issues,
};
use pnpm_lockfile::Lockfile;
use pnpm_reporter::{LogEvent, LogLevel, PeerDependencyIssuesLog};
use std::path::Path;

use crate::InstallError;

/// Report the peer-dependency issues of the lockfile an install just
/// resolved. `None` — every install that skipped resolution — reports
/// nothing.
pub(crate) fn report_peer_dependency_issues<Reporter: pnpm_reporter::Reporter>(
    resolved_lockfile: Option<&Lockfile>,
    lockfile_dir: &Path,
    config: &Config,
) -> Result<(), InstallError> {
    let Some(lockfile) = resolved_lockfile else { return Ok(()) };
    let issues = filter_peer_issues(
        check_all_importers(lockfile, lockfile_dir),
        &config.peer_dependency_rules,
    );
    if !has_reportable_issues(&issues) {
        return Ok(());
    }
    if config.strict_peer_dependencies {
        return Err(InstallError::PeerDependencyIssues {
            rendered: render_peer_issues(&issues),
            hints: hints(&issues),
        });
    }
    Reporter::emit(&LogEvent::PeerDependencyIssues(PeerDependencyIssuesLog {
        level: LogLevel::Debug,
        issues_by_projects: serde_json::to_value(&issues).unwrap_or_default(),
    }));
    Ok(())
}

/// The ways out of the failure, in pnpm's order: auto-installing the
/// peers first when any is absent, then switching the guard off.
fn hints(issues: &IssuesByProjects) -> String {
    let mut hints = Vec::new();
    if has_missing_peers(issues) {
        hints.push(
            r#"To auto-install peer dependencies, add the following to "pnpm-workspace.yaml" in your project root:

  autoInstallPeers: true"#,
        );
    }
    hints.push(
        "To disable failing on peer dependency issues, add the following to pnpm-workspace.yaml in your project root:

  strictPeerDependencies: false",
    );
    hints.join("\n")
}
