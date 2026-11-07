//! The stat of each vouched project manifest and the fields the check reads
//! from it, so the next check can take an unchanged manifest's recorded digest
//! and fields without reading the file.

pub(crate) use stand_ins::{ManifestStandIns, manifest_stand_ins};

use super::{FileStat, RecordValidation, Vouched, manifest_digest, stat_trusted};
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace::{importer_id_from_root_dir, read_exact_project_manifest};
use pnpm_workspace_state::{ManifestFile, Validation};
use std::collections::BTreeMap;

mod stand_ins;

/// The manifest fields the repeat-install check and the project
/// installability check read: a top-level field whole, only one key of an
/// object field, the dependency maps without URL specifiers, or each peer's
/// `optional` flag. A record keeps these, so a manifest rebuilt from them
/// answers both checks the way the file does. Any other value stays out of
/// the workspace state. A change to the list bumps [`FIELDS_REVISION`].
const CHECKED_FIELDS: [(&str, Option<&str>); 15] = [
    ("name", None),
    ("version", None),
    ("dependencies", None),
    ("devDependencies", None),
    ("optionalDependencies", None),
    ("peerDependencies", None),
    ("peerDependenciesMeta", None),
    ("bin", None),
    ("directories", Some("bin")),
    ("publishConfig", Some("directory")),
    ("engines", None),
    ("devEngines", None),
    ("os", None),
    ("cpu", None),
    ("libc", None),
];

/// The revision of [`CHECKED_FIELDS`] a record is written with.
const FIELDS_REVISION: u32 = 2;

/// The fields `file` records, when it records the current [`CHECKED_FIELDS`].
pub(super) fn current_fields(
    file: &ManifestFile,
) -> Option<&serde_json::Map<String, serde_json::Value>> {
    file.fields
        .as_ref()
        .filter(|_| file.fields_revision == FIELDS_REVISION)
}

/// Whether a write would give a `vouched` project a record the next check can
/// use, where `validation` holds none: its record is missing or has no
/// fields, or the manifest's file name or stat differs from it or is not
/// trusted. A manifest that cannot be stat'd, cannot be recorded with
/// fields, or has a stat a write now would not trust, does not count, and
/// neither does a write under `SOURCE_DATE_EPOCH`, which records nothing.
pub(crate) fn records_need_refresh(
    check: &crate::OptimisticRepeatInstallCheck<'_>,
    validation: &Validation,
    vouched: &std::collections::HashSet<std::path::PathBuf>,
) -> bool {
    if check.config.source_date_epoch.is_some() {
        return false;
    }
    let mut filesystem_now_ns = None;
    check.project_manifests
        .iter()
        .filter(|(root_dir, _)| vouched.contains(root_dir))
        .any(|(root_dir, manifest)| {
            let Some(stat) = FileStat::at(manifest.path()) else { return false };
            let importer_id = importer_id_from_root_dir(check.workspace_root, root_dir);
            let usable = validation.manifest_files
                .get(&importer_id)
                .is_some_and(|file| {
                    current_fields(file).is_some()
                        && manifest.path().file_name()
                            == Some(std::ffi::OsStr::new(&file.file_name))
                        && stat.matches_stat(file.mtime_ns, file.ctime_ns, file.size)
                        && stat_trusted(file.mtime_ns, file.ctime_ns, validation.recorded_at_ns)
                });
            !usable && recordable(check, manifest, stat, &mut filesystem_now_ns)
        })
}

/// Whether a write now would record `manifest`, at `stat`, with fields and a
/// trusted stat. `filesystem_now_ns` caches the filesystem clock probe.
fn recordable(
    check: &crate::OptimisticRepeatInstallCheck<'_>,
    manifest: &PackageManifest,
    stat: FileStat,
    filesystem_now_ns: &mut Option<Option<i64>>,
) -> bool {
    if recorded_fields(manifest.value()).is_none() {
        return false;
    }
    let now = *filesystem_now_ns.get_or_insert_with(|| {
        crate::install_state::filesystem_now_ns_in_modules(
            &check.config.workspace_state_modules_dir(check.workspace_root),
        )
    });
    let (mtime_ns, ctime_ns, _) = stat.parts();
    now.is_some_and(|now| stat_trusted(mtime_ns, ctime_ns, now))
}

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
    (same_manifest && trusted && current_fields(file).is_some()).then(|| file.clone())
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

/// The previous record of the manifest, when it has fields, its trusted stat
/// is the current one, and it was recorded with the same digest.
fn reusable_file<'previous>(
    previous: &'previous Validation,
    importer_id: &str,
    (file_name, stat): (&str, FileStat),
    digest: &str,
) -> Option<&'previous ManifestFile> {
    let file = previous.manifest_files.get(importer_id)?;
    let unchanged = current_fields(file).is_some()
        && file.file_name == file_name
        && stat.matches_stat(file.mtime_ns, file.ctime_ns, file.size)
        && stat_trusted(file.mtime_ns, file.ctime_ns, previous.recorded_at_ns)
        && previous.manifests.get(importer_id).map(String::as_str) == Some(digest);
    unchanged.then_some(file)
}

fn manifest_file(manifest: &PackageManifest, file_name: String, stat: FileStat) -> ManifestFile {
    let (mtime_ns, ctime_ns, size) = stat.parts();
    ManifestFile {
        file_name,
        mtime_ns,
        ctime_ns,
        size,
        fields: recorded_fields(manifest.value()),
        fields_revision: FIELDS_REVISION,
    }
}

/// The values of the [`CHECKED_FIELDS`] a record keeps. `None` when a
/// dependency specifier the check reads may hold a credential, so the next
/// check reads the manifest file instead.
fn recorded_fields(
    manifest: &serde_json::Value,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let mut fields = serde_json::Map::new();
    for (field, key) in CHECKED_FIELDS {
        let Some(field_value) = manifest.get(field) else { continue };
        let kept = match (field, key) {
            (_, Some(key)) => match field_value.get(key) {
                Some(value) => serde_json::json!({ key: value }),
                None => continue,
            },
            (
                "dependencies" | "devDependencies" | "optionalDependencies" | "peerDependencies",
                None,
            ) => recorded_dependencies(field_value)?,
            ("peerDependenciesMeta", None) => peer_optional_flags(field_value),
            (_, None) => field_value.clone(),
        };
        fields.insert(field.to_string(), kept);
    }
    Some(fields)
}

/// The specifier schemes the check reads beyond the name a specifier depends
/// on.
const CHECKED_SCHEMES: [&str; 6] = ["file", "link", "workspace", "catalog", "npm", "jsr"];

/// The dependency map as a record keeps it, per [`recorded_specifier`].
fn recorded_dependencies(dependencies: &serde_json::Value) -> Option<serde_json::Value> {
    let Some(dependencies) = dependencies.as_object() else { return Some(dependencies.clone()) };
    dependencies
        .iter()
        .map(|(alias, spec)| {
            let recorded = match spec.as_str() {
                Some(spec) => recorded_specifier(spec)?,
                None => spec.clone(),
            };
            Some((alias.clone(), recorded))
        })
        .collect::<Option<serde_json::Map<_, _>>>()
        .map(Into::into)
}

/// `spec` as a record keeps it, so no credential in it reaches the state:
///
/// - a specifier of a scheme the check does not read becomes `*`. The check
///   reads it only for the name it depends on, which the alias gives;
/// - any other specifier that may hold a credential gives `None`. Replacing
///   it would change what the check reads, such as a local path;
/// - everything else stays as written.
fn recorded_specifier(spec: &str) -> Option<serde_json::Value> {
    let trimmed = spec.trim();
    let rest = match specifier_scheme(trimmed) {
        Some(scheme) if CHECKED_SCHEMES.contains(&scheme.as_str()) => &trimmed[scheme.len() + 1..],
        Some(_) => return Some("*".into()),
        None => trimmed,
    };
    (!may_hold_credential(rest)).then(|| spec.into())
}

/// Whether `text` may hold a credential: it has an `@` after a `:`, the
/// shape of a `user:password@` part or of an SCP-style Git address, or a
/// URL authority with an `@`, the shape of a token given as the user name.
fn may_hold_credential(text: &str) -> bool {
    let after_colon = text
        .find(':')
        .is_some_and(|colon| text[colon..].contains('@'));
    let in_authority = text
        .split_once("//")
        .is_some_and(|(_, rest)| {
            rest.split('/')
                .next()
                .is_some_and(|authority| authority.contains('@'))
        });
    after_colon || in_authority
}

/// The lowercased scheme `spec` starts with, such as `https` or `git+ssh`.
/// A single letter is a Windows drive, not a scheme.
fn specifier_scheme(spec: &str) -> Option<String> {
    let (scheme, _) = spec.split_once(':')?;
    let is_scheme = scheme.len() > 1
        && scheme.starts_with(|first: char| first.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || matches!(char, '+' | '.' | '-'));
    is_scheme.then(|| scheme.to_ascii_lowercase())
}

/// Each peer's `optional` flag, the only part of `peerDependenciesMeta` the
/// check reads.
fn peer_optional_flags(meta: &serde_json::Value) -> serde_json::Value {
    let Some(meta) = meta.as_object() else { return serde_json::Value::Null };
    meta.iter()
        .filter_map(|(name, entry)| {
            let optional = entry.get("optional")?.as_bool()?;
            Some((name.clone(), serde_json::json!({ "optional": optional })))
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

#[cfg(test)]
mod tests;
