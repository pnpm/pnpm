//! The first attempt of the install fast path and of the verify-deps gate:
//! the repeat-install check on manifests rebuilt from the last validation's
//! records, before discovery reads any project manifest.

use super::{
    Config, CoveredProjects, FastPathContext, FastPathProjects, PackageManifest, Path, PathBuf,
    UpToDateFastPathCheck, WorkspaceState, build_project_manifests_list,
    check_fast_path_repeat_install,
};
use crate::install_state::validation::{ManifestStandIns, manifest_stand_ins};
use pnpm_workspace::ProjectManifestPaths;
use std::collections::HashMap;

/// What the check on the stand-ins decided.
pub(super) enum RecordsCheck {
    /// The install is up to date.
    UpToDate(CoveredProjects),
    /// The check on the files decides. Holds the listing the attempt made,
    /// for that check to read.
    Undecided(Option<Vec<ProjectManifestPaths>>),
}

/// The repeat-install check on the stand-ins. It leaves the decision to the
/// check on the files when no workspace manifest drives the install, the
/// state has no manifest records, a project's manifest may have changed
/// since the last validation, or the check would refresh the state.
pub(super) fn check_from_records(
    check: &UpToDateFastPathCheck<'_>,
    context: &FastPathContext<'_>,
    state: &WorkspaceState,
) -> RecordsCheck {
    let listing = list_stand_ins(&ListedWorkspace {
        config: check.config,
        workspace_root: context.workspace_root,
        workspace_manifest: context.workspace_manifest,
        state_root: context.state_root,
        state,
    });
    let (listed, stand_ins) = match listing {
        Listing::StandIns { listed, stand_ins } => (listed, stand_ins),
        Listing::Files(listed) => return RecordsCheck::Undecided(listed),
    };
    let project_manifests = build_project_manifests_list(check.manifest, Some(&stand_ins.projects));
    // The install refuses its shortcut for a malformed dependency declaration,
    // which only a `readPackage` hook that repairs it lets the install pass.
    let malformed = project_manifests
        .iter()
        .any(|(_, manifest)| manifest.validate_dependency_types().is_err());
    if malformed {
        return RecordsCheck::Undecided(Some(listed));
    }
    let recorded_digests = stand_in_digests(&project_manifests, &stand_ins, check.manifest);
    let checked = check_fast_path_repeat_install(
        check,
        context,
        FastPathProjects {
            state,
            manifests: &project_manifests,
            recorded_digests: Some(&recorded_digests),
            bin_projects: None,
        },
    );
    if let Err(reason) = checked {
        tracing::debug!(target: "pacquet::install", reason, "the recorded manifests cannot decide");
        return RecordsCheck::Undecided(Some(listed));
    }
    tracing::debug!(target: "pacquet::install", "the recorded manifests decided the install");
    RecordsCheck::UpToDate(CoveredProjects::of(Some(stand_ins.projects.len()), &project_manifests))
}

/// The workspace a check lists projects in.
pub(super) struct ListedWorkspace<'a> {
    pub(super) config: &'a Config,
    pub(super) workspace_root: &'a Path,
    /// `None` when no `pnpm-workspace.yaml` drives the install.
    pub(super) workspace_manifest: Option<&'a pnpm_workspace::WorkspaceManifest>,
    /// The root the state names importers from.
    pub(super) state_root: &'a Path,
    pub(super) state: &'a WorkspaceState,
}

/// The projects of a workspace, as stand-ins or as the listing to read.
pub(super) enum Listing {
    StandIns {
        listed: Vec<ProjectManifestPaths>,
        stand_ins: ManifestStandIns,
    },
    /// The manifests have to be read. Holds the listing when one was made.
    Files(Option<Vec<ProjectManifestPaths>>),
}

/// The workspace's projects as stand-ins per [`manifest_stand_ins`], or the
/// listing to read when the state has no manifest records or a record cannot
/// stand in. Nothing is listed without a workspace manifest or records.
pub(super) fn list_stand_ins(workspace: &ListedWorkspace<'_>) -> Listing {
    let Some(validation) = workspace.state.validation
        .as_ref()
        .filter(|validation| !validation.manifest_files.is_empty())
    else {
        return Listing::Files(None);
    };
    let Some(workspace_manifest) = workspace.workspace_manifest else {
        return Listing::Files(None);
    };
    let listed = {
        let _span =
            tracing::debug_span!(target: "pacquet::install", "list_workspace_projects").entered();
        let opts = crate::install::checked_projects_opts(workspace.config, workspace_manifest);
        pnpm_workspace::find_workspace_project_manifests(workspace.workspace_root, &opts)
    };
    let Ok(listed) = listed else { return Listing::Files(None) };
    match manifest_stand_ins(workspace.state_root, validation, &listed) {
        Some(stand_ins) => Listing::StandIns { listed, stand_ins },
        None => Listing::Files(Some(listed)),
    }
}

/// The recorded digests of the stand-ins `project_manifests` holds.
/// [`build_project_manifests_list`] puts the active project's manifest,
/// which the command read from disk, in place of its stand-in.
pub(super) fn stand_in_digests(
    project_manifests: &[(PathBuf, &PackageManifest)],
    stand_ins: &ManifestStandIns,
    active_manifest: &PackageManifest,
) -> HashMap<PathBuf, String> {
    project_manifests
        .iter()
        .filter(|(_, manifest)| !std::ptr::eq(*manifest, active_manifest))
        .filter_map(|(root_dir, _)| {
            let digest = stand_ins.digests.get(root_dir)?;
            Some((root_dir.clone(), digest.clone()))
        })
        .collect()
}
