//! Install-time reporting of unmet peer dependencies.
//!
//! Mirrors pnpm's `reportPeerDependencyIssues`: an install that
//! resolved ends by applying `peerDependencyRules` to the issues its
//! resolution left behind, then either failing the install
//! (`strictPeerDependencies`) or logging them for the reporter. An
//! install that did not resolve (a frozen or up-to-date-lockfile run)
//! reports nothing, so `pnpm peers check` is what answers for a tree
//! nobody re-resolved.

use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_deps_inspection_peers::{
    ImporterPeerCheck,
    LinkedLockfileLoader,
    PeerIssuesReport,
    ProjectManifests,
    peer_issues_for_lockfile,
};
use pnpm_lockfile::{
    LoadLockfileError,
    Lockfile,
};
use pnpm_package_manifest::PackageManifest;
use pnpm_reporter::{
    LogEvent,
    LogLevel,
    PeerDependencyIssuesLog,
};
use std::{
    collections::HashSet,
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

use crate::InstallError;

/// Report the peer-dependency issues of the lockfile an install just
/// resolved. `None` (every install that skipped resolution) reports
/// nothing.
///
/// `peer_issue_importer_ids` is the resolution's own verdict: the
/// importers it left an issue under. Only those are walked, so a
/// workspace the resolution found clean costs nothing here. The
/// lockfile stays the report's source; it carries the resolved
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
    let project_manifests = workspace.manifests();
    let check = ImporterPeerCheck {
        lockfile,
        lockfile_dir: workspace.lockfile_dir,
        workspace_dir: config.workspace_dir.as_deref(),
        importer_ids: &importer_ids,
        catalogs: workspace.catalogs,
        resolve_peers_from_workspace_root: config.resolve_peers_from_workspace_root,
        project_manifests: &project_manifests,
    };
    let Some(report) = peer_issues(check, config)? else { return Ok(()) };
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

/// [`peer_issues_for_lockfile`], with a linked sibling's own lockfile
/// loaded where the workspace gives each project one.
fn peer_issues(
    check: ImporterPeerCheck<'_>,
    config: &Config,
) -> Result<Option<PeerIssuesReport>, InstallError> {
    let mut sibling_err = None;
    let mut load_sibling =
        |sibling_dir: &Path, sibling_name: Option<&str>| match load_sibling_lockfile(
            config,
            sibling_dir,
            sibling_name,
        ) {
            Ok(lockfile) => lockfile,
            Err(err) => {
                sibling_err.get_or_insert(err);
                None
            }
        };
    let linked_lockfiles: Option<&mut LinkedLockfileLoader<'_>> =
        has_dedicated_lockfiles(config).then_some(&mut load_sibling);
    let report = peer_issues_for_lockfile(check, &config.peer_dependency_rules, linked_lockfiles)
        .map_err(InstallError::CatalogResolution)?;
    match sibling_err {
        Some(err) => Err(InstallError::LoadWantedLockfile(err)),
        None => Ok(report),
    }
}

/// Every workspace project with its own lockfile, so a linked sibling's
/// peers resolve against that sibling's lockfile.
fn has_dedicated_lockfiles(config: &Config) -> bool {
    config.workspace_dir.is_some() && !config.shares_one_lockfile()
}

fn load_sibling_lockfile(
    config: &Config,
    sibling_dir: &Path,
    sibling_name: Option<&str>,
) -> Result<Option<Arc<Lockfile>>, LoadLockfileError> {
    let mut sibling_config = config.clone();
    sibling_config.anchor_dedicated_project(sibling_dir, sibling_name);
    Ok(Lockfile::load_wanted(sibling_dir, &sibling_config.wanted_lockfile_selection())?
        .map(Arc::new))
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
