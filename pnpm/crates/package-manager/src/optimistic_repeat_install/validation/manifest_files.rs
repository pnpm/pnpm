//! The stat of each vouched project manifest and the fields the check reads
//! from it, so the next check can take an unchanged manifest's recorded digest
//! and fields without reading the file.

use super::{FileStat, RecordValidation, Vouched, manifest_digest, stat_trusted};
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace::{importer_id_from_root_dir, read_exact_project_manifest};
use pnpm_workspace_state::{ManifestFile, Validation};
use std::collections::BTreeMap;

pub(crate) use stand_ins::{ManifestStandIns, manifest_stand_ins};

mod stand_ins;

/// The top-level manifest fields the repeat-install check reads. A record
/// keeps these, so a manifest rebuilt from them answers the check the way
/// the file does.
pub(crate) const CHECKED_FIELDS: [&str; 10] = [
    "name",
    "version",
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
    "peerDependenciesMeta",
    "bin",
    "directories",
    "publishConfig",
];

/// The manifest file of each project the write vouches for, keyed by importer
/// id. Each is stat'd before the caller probes the filesystem clock, and kept
/// only when the file still holds the manifest the write digests: one edited
/// before the stat no longer matches, and one edited after it has a stat the
/// next check rejects. `previous` lets a manifest whose stat and digest are
/// unchanged skip the read.
pub(super) fn stat_vouched(
    inputs: &RecordValidation<'_>,
    previous: Option<&Validation>,
) -> BTreeMap<String, ManifestFile> {
    let vouched = inputs.project_manifests
        .iter()
        .filter(|(root_dir, _)| match inputs.vouched {
            Vouched::All => true,
            Vouched::Projects { dirs, .. } => dirs.contains(root_dir),
        });
    vouched
        .filter_map(|(root_dir, manifest)| {
            let importer_id = importer_id_from_root_dir(inputs.workspace_root, root_dir);
            let file = stat_manifest(manifest, &importer_id, previous)?;
            Some((importer_id, file))
        })
        .collect()
}

/// Moves `stats` into `validation` for the projects it records with the same
/// digest, and carries the file of each other recorded project from
/// `carried` when that record proves the same manifest with a stat `carried`
/// trusts. An untrusted stat stays untrusted: the later `recorded_at_ns` of
/// `validation` must not vouch for an edit that kept it.
pub(super) fn insert(
    validation: &mut Validation,
    mut stats: BTreeMap<String, ManifestFile>,
    carried: Option<&Validation>,
) {
    let recorded = validation.manifests
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    for importer_id in recorded {
        let file =
            stats.remove(&importer_id).or_else(|| carried_file(validation, carried, &importer_id));
        if let Some(file) = file {
            validation.manifest_files.insert(importer_id, file);
        }
    }
}

fn carried_file(
    validation: &Validation,
    carried: Option<&Validation>,
    importer_id: &str,
) -> Option<ManifestFile> {
    let carried = carried?;
    let same_manifest =
        carried.manifests.get(importer_id) == validation.manifests.get(importer_id);
    let file = carried.manifest_files.get(importer_id)?;
    let trusted = stat_trusted(file.mtime_ns, file.ctime_ns, carried.recorded_at_ns);
    (same_manifest && trusted).then(|| file.clone())
}

fn stat_manifest(
    manifest: &PackageManifest,
    importer_id: &str,
    previous: Option<&Validation>,
) -> Option<ManifestFile> {
    let path = manifest.path();
    let file_name = path
        .file_name()?
        .to_string_lossy()
        .into_owned();
    let stat = FileStat::at(path)?;
    let digest = manifest_digest(manifest);
    if let Some(unchanged) = previous.and_then(|previous| {
        reusable_file(previous, importer_id, (&file_name, stat), &digest)
    }) {
        return Some(unchanged.clone());
    }
    let on_disk = read_exact_project_manifest(path).ok()?;
    (manifest_digest(&on_disk) == digest).then(|| manifest_file(manifest, file_name, stat))
}

/// The previous record of the manifest, when its trusted stat is the current
/// one and it was recorded with the same digest.
fn reusable_file<'previous>(
    previous: &'previous Validation,
    importer_id: &str,
    (file_name, stat): (&str, FileStat),
    digest: &str,
) -> Option<&'previous ManifestFile> {
    let file = previous.manifest_files.get(importer_id)?;
    let unchanged = file.file_name == file_name
        && stat.matches_stat(file.mtime_ns, file.ctime_ns, file.size)
        && stat_trusted(file.mtime_ns, file.ctime_ns, previous.recorded_at_ns)
        && previous.manifests.get(importer_id).map(String::as_str) == Some(digest);
    unchanged.then_some(file)
}

fn manifest_file(manifest: &PackageManifest, file_name: String, stat: FileStat) -> ManifestFile {
    let (mtime_ns, ctime_ns, size) = stat.parts();
    let value = manifest.value();
    let fields = CHECKED_FIELDS
        .into_iter()
        .filter_map(|field| Some((field.to_string(), value.get(field)?.clone())))
        .collect();
    ManifestFile { file_name, mtime_ns, ctime_ns, size, fields: Some(fields) }
}
