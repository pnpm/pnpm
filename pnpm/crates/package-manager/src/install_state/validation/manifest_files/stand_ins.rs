//! Project manifests rebuilt from the last validation's records, so the
//! check can run without reading any manifest that did not change.

use super::super::{FileStat, stat_trusted};
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace::{Project, ProjectManifestPaths, importer_id_from_root_dir};
use pnpm_workspace_state::Validation;
use rayon::prelude::*;
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

/// Every listed project, with a manifest rebuilt from its record.
pub(crate) struct ManifestStandIns {
    /// In listing order.
    pub(crate) projects: Vec<Project>,
    /// The recorded digest of each project's manifest, by project directory.
    pub(crate) digests: HashMap<PathBuf, String>,
}

/// Stand-ins for the manifests of the `listed` projects. `None` when any
/// project's manifest may differ from what `validation` records: it has no
/// record, its first existing manifest file is another one, its stat
/// changed or is not trusted, or it cannot be stat'd. A listed directory
/// with no manifest file is not a project and is left out.
pub(crate) fn manifest_stand_ins(
    state_root: &Path,
    validation: &Validation,
    listed: &[ProjectManifestPaths],
) -> Option<ManifestStandIns> {
    let existing = listed
        .par_iter()
        .map(|project| first_existing_manifest(&project.candidates))
        .collect::<Vec<_>>();
    let mut projects = Vec::with_capacity(listed.len());
    let mut digests = HashMap::with_capacity(listed.len());
    for (ProjectManifestPaths { root_dir, .. }, existing) in listed.iter().zip(existing) {
        let Some((path, stat)) = existing? else {
            continue;
        };
        let importer_id = importer_id_from_root_dir(state_root, root_dir);
        let (manifest, digest) = stand_in(validation, &importer_id, path, stat)?;
        digests.insert(root_dir.clone(), digest.to_string());
        projects.push(Project { root_dir: root_dir.clone(), manifest, dependency_manifest: None });
    }
    Some(ManifestStandIns { projects, digests })
}

/// The manifest rebuilt from the record of `importer_id`, and its recorded
/// digest, when the file at `path` still has the recorded, trusted stat.
fn stand_in<'validation>(
    validation: &'validation Validation,
    importer_id: &str,
    path: PathBuf,
    stat: FileStat,
) -> Option<(PackageManifest, &'validation str)> {
    let file = validation.manifest_files.get(importer_id)?;
    let digest = validation.manifests.get(importer_id)?;
    let fields = super::current_fields(file)?;
    let unchanged = path.file_name()? == file.file_name.as_str()
        && stat.matches_stat(file.mtime_ns, file.ctime_ns, file.size)
        && stat_trusted(file.mtime_ns, file.ctime_ns, validation.recorded_at_ns);
    unchanged.then(|| {
        (
            PackageManifest::from_value(path, serde_json::Value::Object(fields.clone())),
            digest.as_str(),
        )
    })
}

/// The first of `candidates` that exists, as discovery would read it.
/// `Some(None)` when none exists, and `None` when one cannot be stat'd or
/// is not a file, which discovery would fail on.
fn first_existing_manifest(candidates: &[PathBuf]) -> Option<Option<(PathBuf, FileStat)>> {
    for path in candidates {
        match fs::metadata(path) {
            Ok(metadata) if metadata.is_file() => {
                return FileStat::of(path, &metadata).map(|stat| Some((path.clone(), stat)));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Ok(_) | Err(_) => return None,
        }
    }
    Some(None)
}
