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
use pnpm_deps_inspection_peers::{ProjectManifests, peer_issues_for_lockfile};
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use pnpm_reporter::{LogEvent, LogLevel, PeerDependencyIssuesLog};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

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
pub(crate) fn report_peer_dependency_issues<Reporter: pnpm_reporter::Reporter>(
    resolved_lockfile: Option<&Lockfile>,
    peer_issue_importer_ids: &HashSet<String>,
    installed_importer_ids: &HashSet<String>,
    workspace: PeerReportWorkspace<'_>,
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
        workspace.lockfile_dir,
        &importer_ids,
        &config.peer_dependency_rules,
        workspace.catalogs,
        config.resolve_peers_from_workspace_root,
        &workspace.manifests(),
    )
    .map_err(InstallError::CatalogResolution)?
    else {
        return Ok(());
    };
    if config.strict_peer_dependencies {
        return Err(InstallError::PeerDependencyIssues {
            rendered: Reporter::report_fatal_error(report.render_error()),
        });
    }
    Reporter::emit(&LogEvent::PeerDependencyIssues(PeerDependencyIssuesLog {
        level: LogLevel::Debug,
        issues_by_projects: serde_json::to_value(report.issues()).unwrap_or_default(),
    }));
    Ok(())
}

/// The workspace an install's peer report is checked against.
#[derive(Clone, Copy)]
pub(crate) struct PeerReportWorkspace<'a> {
    pub(crate) lockfile_dir: &'a Path,
    /// `None` when the install has no catalog context. Raw `catalog:` peer
    /// ranges in externally linked packages then remain peer issues instead
    /// of becoming catalog-configuration errors.
    pub(crate) catalogs: Option<&'a Catalogs>,
    /// The manifests the install read, before `packageExtensions` and
    /// `readPackage`. A `link:` to one of these projects is checked without
    /// reading its manifest again.
    pub(crate) project_manifests: &'a [(PathBuf, &'a PackageManifest)],
}

impl<'a> PeerReportWorkspace<'a> {
    fn manifests(&self) -> ProjectManifests<'a> {
        self.project_manifests
            .iter()
            .map(|(project_dir, manifest)| {
                (
                    pnpm_workspace::importer_id_from_root_dir(self.lockfile_dir, project_dir),
                    *manifest,
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
