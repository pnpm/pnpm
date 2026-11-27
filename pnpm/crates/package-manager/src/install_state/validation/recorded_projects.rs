use super::{
    RecordValidation,
    Vouched,
    install_input_digest,
    manifest_digest,
    project_bins_digest,
    workspace_bins_digest,
};
use crate::install_state::settings::settings_json;
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace::importer_id_from_root_dir;
use pnpm_workspace_state::{
    InstallInputs,
    Validation,
};
use std::collections::BTreeMap;

struct RecordedProject {
    importer_id: String,
    manifest: String,
    input_manifest: Option<String>,
    lockfile: Option<String>,
    bins: Option<String>,
    vouched: bool,
}

struct ProjectInputs<'a> {
    manifest: &'a PackageManifest,
    importer_id: String,
    vouched: bool,
    bins: Option<&'a str>,
    lockfile: Option<&'a str>,
    carried: Option<&'a Validation>,
}

impl ProjectInputs<'_> {
    fn record(self) -> Option<RecordedProject> {
        if self.vouched {
            let project_bins = project_bins_digest(&self.importer_id, self.manifest, self.bins);
            return Some(RecordedProject {
                importer_id: self.importer_id,
                manifest: manifest_digest(self.manifest),
                input_manifest: Some(install_input_digest(self.manifest)),
                lockfile: self.lockfile.map(str::to_string),
                bins: project_bins.map(str::to_string),
                vouched: true,
            });
        }
        let carried = self.carried?;
        let record = carried.project(&self.importer_id)?;
        let bins = carried.workspace_bins.get(&self.importer_id).cloned();
        Some(RecordedProject {
            importer_id: self.importer_id,
            manifest: record.manifest.to_string(),
            input_manifest: None,
            lockfile: record.lockfile.map(str::to_string),
            bins,
            vouched: false,
        })
    }
}

struct ProjectRecords<'a> {
    validation: &'a mut Validation,
    carried: Option<&'a Validation>,
    settings: &'a str,
    files: &'a BTreeMap<String, String>,
}

impl ProjectRecords<'_> {
    fn insert(&mut self, record: RecordedProject) {
        let RecordedProject {
            importer_id,
            manifest,
            input_manifest,
            lockfile,
            bins,
            vouched,
        } = record;
        let recorded_inputs = if vouched {
            input_manifest.map(|manifest| InstallInputs {
                settings: self.settings.to_string(),
                manifest,
                lockfile: lockfile.clone(),
                workspace_bins: bins.clone(),
                files: self.files.clone(),
            })
        } else {
            self.carried.and_then(|validation| validation.inputs.get(&importer_id).cloned())
        };
        if let Some(bins) = bins {
            self.validation.workspace_bins.insert(importer_id.clone(), bins);
        }
        if let Some(inputs) = recorded_inputs {
            self.validation.inputs.insert(importer_id.clone(), inputs);
        }
        self.validation.insert_project(importer_id, manifest, lockfile.as_deref());
    }
}

/// Record in `validation` each project the write vouches for, proven against
/// `lockfile`, and each other one `carried` still proves. Returns how many it
/// recorded.
pub(super) fn record_projects(
    validation: &mut Validation,
    inputs: &RecordValidation<'_>,
    carried: Option<&Validation>,
    lockfile: Option<&str>,
    files: &BTreeMap<String, String>,
    settings: &pnpm_workspace_state::WorkspaceStateSettings,
) -> usize {
    let bins = workspace_bins_digest(
        inputs.workspace_root,
        inputs.workspace_projects.unwrap_or(inputs.project_manifests.as_slice()),
    );
    let settings = settings_json(settings);
    let records = inputs.project_manifests
        .iter()
        .filter_map(|(root_dir, manifest)| {
            let vouched = match inputs.vouched {
                Vouched::All => true,
                Vouched::Projects { dirs, .. } => dirs.contains(root_dir),
            };
            ProjectInputs {
                manifest,
                importer_id: importer_id_from_root_dir(inputs.workspace_root, root_dir),
                vouched,
                bins: bins.as_deref(),
                lockfile,
                carried,
            }
            .record()
        })
        .collect::<Vec<_>>();
    let recorded = records.len();
    let mut project_records = ProjectRecords { validation, carried, settings: &settings, files };
    for record in records {
        project_records.insert(record);
    }
    recorded
}
