//! The fast path's first attempt: the repeat-install check on manifests
//! rebuilt from the last validation's records, before discovery reads any
//! project manifest.

use super::{
    FastPathContext, FastPathProjects, PackageManifest, PathBuf, UpToDateFastPathCheck,
    WorkspaceState, build_project_manifests_list, check_fast_path_repeat_install,
};
use crate::optimistic_repeat_install::validation::{ManifestStandIns, manifest_stand_ins};
use pnpm_workspace::ProjectManifestPaths;
use std::collections::HashMap;

/// What the check on the stand-ins decided.
pub(super) enum RecordsCheck {
    /// The install is up to date. Holds the number of workspace projects.
    UpToDate(usize),
    /// The check on the files decides. Holds the listing the attempt made,
    /// for that check to read.
    Undecided(Option<Vec<ProjectManifestPaths>>),
}

/// The repeat-install check on the stand-ins. It leaves the decision to the
/// check on the files when no workspace manifest drives the install, a
/// project's manifest may have changed since the last validation, or the
/// check would refresh the state.
pub(super) fn check_from_records(
    check: &UpToDateFastPathCheck<'_>,
    context: &FastPathContext<'_>,
    state: &WorkspaceState,
) -> RecordsCheck {
    let Some(listed) = list_projects(check, context) else {
        return RecordsCheck::Undecided(None);
    };
    let Some(stand_ins) = state.validation
        .as_ref()
        .and_then(|validation| manifest_stand_ins(context.state_root, validation, &listed))
    else {
        return RecordsCheck::Undecided(Some(listed));
    };
    let project_manifests = build_project_manifests_list(check.manifest, Some(&stand_ins.projects));
    let recorded_digests = stand_in_digests(&project_manifests, &stand_ins, check.manifest);
    let checked = check_fast_path_repeat_install(
        check,
        context,
        FastPathProjects {
            state,
            manifests: &project_manifests,
            recorded_digests: Some(&recorded_digests),
        },
    );
    if let Err(reason) = checked {
        tracing::debug!(target: "pacquet::install", reason, "the recorded manifests cannot decide");
        return RecordsCheck::Undecided(Some(listed));
    }
    tracing::debug!(target: "pacquet::install", "the recorded manifests decided the install");
    RecordsCheck::UpToDate(stand_ins.projects.len())
}

/// [`pnpm_workspace::find_workspace_project_manifests`] for the workspace
/// manifest, inside the `list_workspace_projects` span.
fn list_projects(
    check: &UpToDateFastPathCheck<'_>,
    context: &FastPathContext<'_>,
) -> Option<Vec<ProjectManifestPaths>> {
    let _span =
        tracing::debug_span!(target: "pacquet::install", "list_workspace_projects").entered();
    let opts = super::super::lifecycle::workspace_projects_opts(
        context.workspace_manifest?,
        &check.config.managed_directories(),
    );
    pnpm_workspace::find_workspace_project_manifests(context.workspace_root, &opts).ok()
}

/// The recorded digests of the stand-ins `project_manifests` holds.
/// [`build_project_manifests_list`] puts the active project's manifest,
/// which the command read from disk, in place of its stand-in.
fn stand_in_digests(
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
