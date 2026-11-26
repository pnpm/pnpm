//! The inputs each held project was last installed with.

use super::{
    super::{
        ManifestStat,
        OptimisticRepeatInstallCheck,
        WorkspaceState,
        validation::{
            FileDigests,
            manifest_digest,
            project_bins_digest,
            shared_input_paths,
            workspace_bins_digest,
        },
    },
    GatePolicy,
    InputDrift,
    NewerPatch,
    Stale,
};
use crate::install_state::{
    current_settings_with_catalogs,
    settings_json,
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

pub(super) fn check_recorded_inputs(
    check: &OptimisticRepeatInstallCheck<'_>,
    state: &WorkspaceState,
    drift: &InputDrift<'_>,
    held: &[&ManifestStat<'_>],
    policy: &GatePolicy<'_>,
) -> Result<(), Stale> {
    let Some(validation) = state.validation.as_ref() else {
        return Ok(());
    };
    if validation.inputs.is_empty() {
        return Ok(());
    }
    let current_settings = current_settings_with_catalogs(
        check.config,
        check.layout.node_linker,
        check.layout.included,
        check.layout.supported_architectures,
        check.catalogs,
    );
    let current = current_inputs(check, state, drift, settings_json(&current_settings));
    let patches: HashSet<PathBuf> = check.config.patch_file_paths().collect();
    for stat in held {
        let importer_id = importer_id_from_root_dir(check.workspace_root, stat.root_dir);
        let (Some(recorded), Some(now)) =
            (validation.inputs.get(&importer_id), current.get(&importer_id))
        else {
            continue;
        };
        let matches = inputs_match(
            recorded,
            now,
            &current_settings,
            policy,
            &patches,
            check.config.workspace_dir.as_deref(),
        )?;
        if !matches {
            return Err(Stale::InputsChanged(importer_id));
        }
    }
    Ok(())
}

fn inputs_match(
    recorded: &InstallInputs,
    current: &InstallInputs,
    current_settings: &WorkspaceStateSettings,
    policy: &GatePolicy<'_>,
    patches: &HashSet<PathBuf>,
    workspace_dir: Option<&Path>,
) -> Result<bool, Stale> {
    let files_match = if matches!(policy.newer_patch, NewerPatch::CompareHash) {
        visible_files(&recorded.files, patches).eq(visible_files(&current.files, patches))
    } else {
        recorded.files == current.files
    };
    if recorded.manifest != current.manifest
        || recorded.lockfile != current.lockfile
        || recorded.workspace_bins != current.workspace_bins
        || !files_match
    {
        return Ok(false);
    }
    settings_match(&recorded.settings, current_settings, policy.ignored_settings, workspace_dir)
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
    drift: &InputDrift<'_>,
    settings: String,
) -> BTreeMap<String, InstallInputs> {
    let bins = workspace_bins_digest(check.workspace_root, check.bin_projects());
    let digests = FileDigests::new(state.validation.as_ref());
    let files = shared_input_paths(check.config, &state.pnpmfiles)
        .into_iter()
        .filter_map(|path| {
            let digest = digests.of(&path).digest()?.to_string();
            Some((path.to_string_lossy().into_owned(), digest))
        })
        .collect::<BTreeMap<_, _>>();
    drift.stats
        .iter()
        .map(|stat| {
            let importer_id = importer_id_from_root_dir(check.workspace_root, stat.root_dir);
            let manifest = check.project_manifests
                .recorded_digest(stat.root_dir)
                .map(str::to_string)
                .unwrap_or_else(|| manifest_digest(stat.manifest));
            let workspace_bins = project_bins_digest(&importer_id, stat.manifest, bins.as_deref())
                .map(str::to_string);
            (
                importer_id,
                InstallInputs {
                    settings: settings.clone(),
                    manifest,
                    lockfile: drift.lockfile.digest().map(str::to_string),
                    workspace_bins,
                    files: files.clone(),
                },
            )
        })
        .collect()
}
