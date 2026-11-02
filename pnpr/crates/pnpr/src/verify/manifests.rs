//! Re-deriving the install-critical fields of hosted versions from their
//! stored archives, for versions published before publish-time manifest
//! verification and as a check of the ones published since.

use pnpm_package_manifest::{
    ArchiveFacts, INSTALL_MANIFEST_PROFILE, InstallManifest, parse_manifest_bytes,
};
use pnpm_tarball::{PublicationArchive, read_publication_archive};
use serde_json::Value;
use std::io::Read;

/// The most a stored archive may decode to while it is re-derived: what any
/// `.tgz` may decode to on publish.
const MAX_DECODED_TAR_BYTES: u64 = (4 * 1024 + 256) * 1024 * 1024;

/// The stored field that records how a version's install-critical fields were
/// derived at publish.
const ARCHIVE_MANIFEST_FIELD: &str = "_archiveManifest";

/// How the stored metadata of `version` (`stored`) disagrees with the
/// `package.json` in its archive `tar`, an uncompressed tar stream, under
/// [`INSTALL_MANIFEST_PROFILE`]. `None` when they agree.
pub(super) fn manifest_disagreement(
    package: &str,
    version: &str,
    stored: &Value,
    tar: impl Read,
) -> Option<String> {
    let label = format!("{package}@{version}");
    let archive = match read_publication_archive(tar.take(MAX_DECODED_TAR_BYTES)) {
        Ok(archive) => archive,
        Err(err) => return Some(format!("{label}: {err}")),
    };
    let Some(manifest) = parse_manifest_bytes(&archive.manifest).ok().filter(Value::is_object)
    else {
        return Some(format!("{label}: the archive's package.json is not a JSON object"));
    };
    let facts = ArchiveFacts {
        binding_gyp: archive.binding_gyp,
        has_shrinkwrap: archive.shrinkwrap_integrity.is_some(),
        files: &archive.files,
    };
    let derived = InstallManifest::derive(&manifest, facts);
    let comparison = InstallManifest::derive(stored, facts).compare(&derived);
    let mut problems = comparison.mismatched;
    problems.extend(comparison.tolerated);
    problems.extend(record_problem(stored, &archive));
    if problems.is_empty() {
        return None;
    }
    let state = if stored.get(ARCHIVE_MANIFEST_FIELD).is_some() {
        "verified at publish"
    } else {
        "not verified at publish"
    };
    Some(format!("{label} ({state}): {}", problems.join(", ")))
}

/// What the publish-time record says that the archive no longer does.
fn record_problem(stored: &Value, archive: &PublicationArchive) -> Vec<String> {
    let Some(record) = stored.get(ARCHIVE_MANIFEST_FIELD) else { return Vec::new() };
    let shrinkwrap = archive.shrinkwrap_integrity.as_ref().map(ToString::to_string);
    let expected = [
        ("profile", Some(INSTALL_MANIFEST_PROFILE.to_string())),
        ("packageJson", Some(archive.manifest_integrity.to_string())),
        ("npmShrinkwrap", shrinkwrap),
    ];
    expected
        .into_iter()
        .filter(|(field, value)| record.get(*field).and_then(Value::as_str) != value.as_deref())
        .map(|(field, _)| format!("{ARCHIVE_MANIFEST_FIELD}.{field}"))
        .collect()
}
