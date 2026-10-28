use super::{
    Config, PackageManifest, Path, PathBuf, Validation, WorkspaceState, importer_id_from_root_dir,
    value_digest,
};
use crate::optimistic_repeat_install::{ManifestStat, OptimisticRepeatInstallCheck};

pub(crate) fn workspace_bins_digest(
    workspace_root: &Path,
    projects: &[(PathBuf, &PackageManifest)],
) -> Option<String> {
    let bins: serde_json::Map<String, serde_json::Value> = projects
        .iter()
        .filter_map(|(dir, manifest)| {
            let manifest = manifest.value();
            let bin = manifest
                .get("bin")
                .filter(|value| !value.is_null());
            let directory = manifest
                .get("directories")
                .and_then(|dirs| dirs.get("bin"))
                .filter(|value| !value.is_null());
            if bin.is_none() && directory.is_none() {
                return None;
            }
            Some((
                importer_id_from_root_dir(workspace_root, dir),
                serde_json::json!({
                    "name": manifest.get("name"),
                    "bin": bin,
                    "directory": directory,
                }),
            ))
        })
        .collect();
    (!bins.is_empty()).then(|| value_digest(&serde_json::Value::Object(bins)))
}

/// The bins digest `importer_id` records: `bins` when the project links
/// workspace bins, which the root and any project that
/// [`links_workspace_commands`] do, and `None` otherwise.
pub(crate) fn project_bins_digest<'a>(
    importer_id: &str,
    manifest: &PackageManifest,
    bins: Option<&'a str>,
) -> Option<&'a str> {
    if importer_id == "." || links_workspace_commands(manifest) { bins } else { None }
}

/// Whether the project declares a dependency or a peer, through which a
/// workspace project's commands reach its `node_modules/.bin`.
pub(crate) fn links_workspace_commands(manifest: &PackageManifest) -> bool {
    crate::optimistic_repeat_install::manifest_has_runtime_deps(manifest)
        || manifest
            .value()
            .get("peerDependencies")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|peers| !peers.is_empty())
}

/// Whether `recorded` proves `importer_id` installed with the bins digest
/// `bins`. A state without a validation record proves nothing about any
/// project, so it matches and leaves the verdict to the checks that handle
/// such a state.
pub(crate) fn project_bins_match(
    recorded: Option<&Validation>,
    importer_id: &str,
    bins: Option<&str>,
) -> bool {
    let Some(validation) = recorded else { return true };
    validation.workspace_bins.get(importer_id).map(String::as_str) == bins
}

pub(crate) fn held_workspace_bins_match(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    held: &[&ManifestStat<'_>],
) -> bool {
    if held.is_empty() || state.validation.is_none() {
        return true;
    }
    let bins = workspace_bins_digest(check.workspace_root, check.bin_projects());
    held.iter()
        .all(|stat| {
            let id = importer_id_from_root_dir(check.workspace_root, stat.root_dir);
            project_bins_match(
                state.validation.as_ref(),
                &id,
                project_bins_digest(&id, stat.manifest, bins.as_deref()),
            )
        })
}

/// [`crate::RepeatInstallManifests::workspace_projects`] for an install with
/// dedicated lockfiles whose projects link workspace commands. `None` with a
/// shared lockfile, whose projects already are the whole workspace.
pub(crate) fn dedicated_bin_projects<'w>(
    config: &Config,
    project_manifests: &[(PathBuf, &PackageManifest)],
    workspace_projects: Option<&'w [pnpm_workspace::Project]>,
) -> Option<Vec<(PathBuf, &'w PackageManifest)>> {
    let links = project_manifests.iter().any(|(_, manifest)| links_workspace_commands(manifest));
    if config.shares_one_lockfile() || !links {
        return None;
    }
    let projects = workspace_projects?
        .iter()
        .map(|project| (project.root_dir.clone(), &project.manifest))
        .collect();
    Some(projects)
}
