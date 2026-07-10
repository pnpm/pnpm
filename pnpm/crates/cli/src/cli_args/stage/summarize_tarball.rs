//! Summarize a packed tarball's bytes into the [`PublishSummary`] shape that
//! `pnpm publish --json` emits — used by `pnpm stage download` to describe a
//! staged tarball without re-packing it.

use std::{collections::BTreeSet, io::Read};

use flate2::read::GzDecoder;
use miette::{Context, IntoDiagnostic};
use pacquet_pack::sort_paths_en_locale;
use pacquet_publish::{PackedPkgInfo, PublishSummary, create_publish_summary};
use pacquet_resolving_parse_wanted_dependency::is_valid_old_npm_package_name;
use serde_json::Value;

use super::StageError;

/// Parse a packed (gzipped or plain) tarball and return its
/// [`PublishSummary`]. The tarball must contain a parseable
/// `package/package.json` with a name and version.
pub(super) fn summarize_tarball(tarball_data: &[u8]) -> miette::Result<PublishSummary> {
    let tar_bytes = maybe_gunzip(tarball_data)?;
    let mut archive = tar::Archive::new(tar_bytes.as_slice());
    let mut files: Vec<String> = Vec::new();
    let mut bundled: BTreeSet<String> = BTreeSet::new();
    let mut manifest: Option<Value> = None;
    let mut unpacked_size: u64 = 0;

    let entries =
        archive.entries().into_diagnostic().wrap_err("read the staged tarball's entries")?;
    for entry in entries {
        let mut entry = entry.into_diagnostic().wrap_err("read a staged tarball entry")?;
        let path = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        if entry.header().entry_type().is_file() {
            unpacked_size += entry.header().size().unwrap_or(0);
            files.push(path.strip_prefix("package/").unwrap_or(&path).to_owned());
            if let Some(name) = bundled_dependency_name(&path) {
                bundled.insert(name);
            }
        }
        if path == "package/package.json" {
            let mut text = String::new();
            entry
                .read_to_string(&mut text)
                .into_diagnostic()
                .wrap_err("read package/package.json from the staged tarball")?;
            manifest =
                Some(serde_json::from_str(&text).map_err(|_| StageError::TarballManifestNotFound)?);
        }
    }

    let manifest = manifest.ok_or(StageError::TarballManifestNotFound)?;
    let name = manifest_string(&manifest, "name");
    let version = manifest_string(&manifest, "version");
    if name.is_empty() || version.is_empty() {
        return Err(StageError::TarballManifestNotFound.into());
    }

    sort_paths_en_locale(&mut files);
    let filename = create_tarball_filename(&name, &version, None)?;
    let mut summary = create_publish_summary(
        &PackedPkgInfo {
            published_manifest: &manifest,
            tarball_path: &filename,
            contents: &files,
            unpacked_size,
        },
        tarball_data,
    );
    // A registry-packed tarball's manifest carries an `_id`; prefer it over
    // the derived `name@version`, matching pnpm's `summarizeTarball`.
    if let Some(id) = manifest.get("_id").and_then(Value::as_str) {
        id.clone_into(&mut summary.id);
    }
    if !bundled.is_empty() {
        summary.bundled = bundled.into_iter().collect();
    }
    Ok(summary)
}

/// The safe tarball basename `<normalized-name>-<version>[-<suffix>].tgz`,
/// after validating that the name and version cannot smuggle path segments.
pub(super) fn create_tarball_filename(
    name: &str,
    version: &str,
    suffix: Option<&str>,
) -> Result<String, StageError> {
    if !is_valid_old_npm_package_name(name) {
        return Err(StageError::InvalidPackageName { name: name.to_owned() });
    }
    if version.parse::<node_semver::Version>().is_err() {
        return Err(StageError::InvalidPackageVersion { version: version.to_owned() });
    }
    let suffix = suffix.map(|suffix| format!("-{suffix}")).unwrap_or_default();
    let filename = format!("{}-{version}{suffix}.tgz", normalize_package_name(name));
    // The name/version validation above should already exclude separators;
    // reject outright if a validated component still smuggled one in.
    if filename.contains(['/', '\\', ':']) {
        return Err(StageError::InvalidTarballFilename { filename });
    }
    Ok(filename)
}

/// `@scope/name` → `scope-name`: drop the first `@`, turn the first `/` into
/// a `-`, exactly like pnpm's `normalizePackageName`.
fn normalize_package_name(name: &str) -> String {
    name.replacen('@', "", 1).replacen('/', "-", 1)
}

/// The bundled-dependency name of a `package/node_modules/...` tarball entry:
/// the first path segment (or two for a scoped package) after
/// `node_modules/`.
fn bundled_dependency_name(path: &str) -> Option<String> {
    let rest = path.strip_prefix("package/node_modules/")?;
    let mut segments = rest.split('/');
    let first = segments.next().filter(|segment| !segment.is_empty())?;
    if let Some(scope) = first.strip_prefix('@') {
        if scope.is_empty() {
            return None;
        }
        let second = segments.next().filter(|segment| !segment.is_empty())?;
        return Some(format!("{first}/{second}"));
    }
    Some(first.to_owned())
}

/// Cap on the decompressed tarball; the bytes come from the registry, so a
/// gzip bomb must not inflate past this into memory.
const MAX_DECOMPRESSED_TARBALL_BYTES: u64 = 512 * 1024 * 1024;

/// Gunzip `data`, falling back to the raw bytes when it is not gzipped.
/// Decompression past [`MAX_DECOMPRESSED_TARBALL_BYTES`] is an error.
fn maybe_gunzip(data: &[u8]) -> Result<Vec<u8>, StageError> {
    let mut decoder = GzDecoder::new(data).take(MAX_DECOMPRESSED_TARBALL_BYTES + 1);
    let mut decompressed = Vec::new();
    match decoder.read_to_end(&mut decompressed) {
        Ok(_) if decompressed.len() as u64 > MAX_DECOMPRESSED_TARBALL_BYTES => {
            Err(StageError::RequestFailed {
                operation: "read the staged tarball".to_owned(),
                reason: format!(
                    "tarball exceeded {MAX_DECOMPRESSED_TARBALL_BYTES} bytes when decompressed",
                ),
            })
        }
        Ok(_) => Ok(decompressed),
        Err(_) => Ok(data.to_vec()),
    }
}

fn manifest_string(manifest: &Value, key: &str) -> String {
    manifest.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}
