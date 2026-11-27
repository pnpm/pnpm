//! The inputs each held project was last installed with.

use super::{
    super::{
        ManifestStat,
        OptimisticRepeatInstallCheck,
        WorkspaceState,
        validation::{
            FileDigests,
            install_input_digest,
            project_bins_digest,
            shared_input_paths,
            workspace_bins_digest,
        },
    },
    GatePolicy,
    NewerPatch,
    ProjectStatus,
    Stale,
};
use crate::install_state::{
    current_settings_with_catalogs,
    settings_match,
};
use pnpm_workspace::importer_id_from_root_dir;
use pnpm_workspace_state::{
    InstallInputs,
    WorkspaceStateSettings,
};
use std::{
    collections::{
        BTreeMap,
        HashSet,
    },
    path::{
        Path,
        PathBuf,
    },
};

struct ComparedInputs {
    manifest: String,
    workspace_bins: Option<String>,
}

struct CurrentInputs {
    projects: BTreeMap<String, ComparedInputs>,
    files: BTreeMap<String, String>,
    settings: WorkspaceStateSettings,
}

pub(super) fn check_recorded_inputs(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    held: &[&ManifestStat<'_>],
    policy: &GatePolicy<'_>,
) -> Result<(), Stale> {
    let Some(validation) = state.validation.as_ref() else {
        return Ok(());
    };
    if validation.inputs.is_empty() {
        return Ok(());
    }
    if held.is_empty() {
        return Ok(());
    }
    let current = current_inputs(check, state, held);
    let patches: HashSet<PathBuf> = check.config.patch_file_paths().collect();
    for stat in held {
        let importer_id = importer_id_from_root_dir(check.workspace_root, stat.root_dir);
        let (Some(recorded), Some(now)) =
            (validation.inputs.get(&importer_id), current.projects.get(&importer_id))
        else {
            continue;
        };
        let matches = inputs_match(
            recorded,
            now,
            &current,
            policy,
            &patches,
            check.config.workspace_dir.as_deref(),
            stat.status,
        )?;
        if !matches {
            return Err(Stale::InputsChanged(importer_id));
        }
    }
    Ok(())
}

fn inputs_match(
    recorded: &InstallInputs,
    current: &ComparedInputs,
    shared: &CurrentInputs,
    policy: &GatePolicy<'_>,
    patches: &HashSet<PathBuf>,
    workspace_dir: Option<&Path>,
    status: ProjectStatus,
) -> Result<bool, Stale> {
    let files_match = if matches!(policy.newer_patch, NewerPatch::CompareHash) {
        visible_files(&recorded.files, patches).eq(visible_files(&shared.files, patches))
    } else {
        recorded.files.iter().eq(shared.files.iter())
    };
    // The content check has just proved this manifest against the changed
    // lockfile, so the proof recorded under the old lockfile cannot hold it.
    let manifest_matches =
        status == ProjectStatus::LockfileChanged || recorded.manifest == current.manifest;
    if !manifest_matches || recorded.workspace_bins != current.workspace_bins || !files_match {
        return Ok(false);
    }
    settings_match(&recorded.settings, &shared.settings, policy.ignored_settings, workspace_dir)
        .map_err(|_| Stale::Unprovable)
}

fn visible_files<'a>(
    files: &'a BTreeMap<String, String>,
    patches: &'a HashSet<PathBuf>,
) -> impl Iterator<Item = (&'a String, &'a String)> {
    files
        .iter()
        .filter(move |(path, _)| !patches.contains(Path::new(path)))
}

fn current_inputs(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    held: &[&ManifestStat<'_>],
) -> CurrentInputs {
    let settings = current_settings_with_catalogs(
        check.config,
        check.layout.node_linker,
        check.layout.included,
        check.layout.supported_architectures,
        check.catalogs,
    );
    let bins = workspace_bins_digest(check.workspace_root, check.bin_projects());
    let digests = FileDigests::new(state.validation.as_ref());
    let files = shared_input_paths(check.config, &state.pnpmfiles)
        .into_iter()
        .filter_map(|path| {
            let digest = digests.of(&path).digest()?.to_string();
            Some((path.to_string_lossy().into_owned(), digest))
        })
        .collect::<BTreeMap<_, _>>();
    let projects = held
        .iter()
        .map(|stat| {
            let importer_id = importer_id_from_root_dir(check.workspace_root, stat.root_dir);
            let manifest = install_input_digest(stat.manifest);
            let workspace_bins = project_bins_digest(&importer_id, stat.manifest, bins.as_deref())
                .map(str::to_string);
            (importer_id, ComparedInputs { manifest, workspace_bins })
        })
        .collect();
    CurrentInputs { projects, files, settings }
}
