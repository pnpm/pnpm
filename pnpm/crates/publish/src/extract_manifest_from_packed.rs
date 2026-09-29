//! read `package/package.json` out of
//! a pre-built `.tgz` so a tarball passed to `pnpm publish <tarball>` can be
//! published without repacking.

use std::{
    fs::File,
    io::{self, Read, Seek},
    path::Path,
};

use flate2::read::GzDecoder;
use pnpm_diagnostics::miette::{self, Diagnostic};
use pnpm_package_manifest::{is_markdown_readme_file_name, is_readme_file_name, parse_manifest};
use serde_json::Value;

const TARBALL_SUFFIXES: [&str; 2] = [".tar.gz", ".tgz"];

/// Whether `path` looks like a publishable tarball (ends with `.tar.gz` or
/// `.tgz`).
#[must_use]
pub fn is_tarball_path(path: &str) -> bool {
    TARBALL_SUFFIXES
        .iter()
        .any(|suffix| path.ends_with(suffix))
}

/// Read and parse `package/package.json` from the gzipped tarball at
/// `tarball_path`.
pub fn extract_manifest_from_packed(tarball_path: &str) -> Result<Value, ExtractManifestError> {
    let read_err = |source: std::io::Error| ExtractManifestError::Read {
        tarball_path: tarball_path.to_owned(),
        source,
    };
    let file = File::open(tarball_path).map_err(read_err)?;
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    let entries = archive.entries().map_err(read_err)?;

    for entry in entries {
        let mut entry = entry.map_err(read_err)?;
        let path = entry.path().map_err(read_err)?;
        if normalize_entry_path(&path) != "package/package.json" {
            continue;
        }
        let mut text = String::new();
        entry.read_to_string(&mut text).map_err(read_err)?;
        return parse_manifest(&text)
            .map_err(|source| ExtractManifestError::Parse {
                tarball_path: tarball_path.to_owned(),
                source,
            });
    }

    Err(ExtractManifestError::MissingManifest(PublishArchiveMissingManifestError {
        tarball_path: tarball_path.to_owned(),
    }))
}

/// Read the publish manifest from a pre-built tarball, filling in its `readme`
/// from the tarball's root README when the manifest doesn't already declare
/// one. This mirrors the npm CLI, which reads the readme out of the tarball (via
/// pacote's `fullReadJson`) so the registry gets it as metadata even though it
/// isn't stored in the packed `package.json`.
pub fn extract_publish_manifest_from_packed(
    tarball_path: &str,
) -> Result<Value, ExtractManifestError> {
    let read_err = |source: std::io::Error| ExtractManifestError::Read {
        tarball_path: tarball_path.to_owned(),
        source,
    };
    let file = File::open(tarball_path).map_err(read_err)?;
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    let PackedEntries {
        manifest_text,
        preferred_readme,
        fallback_candidate,
    } = scan_packed_entries(&mut archive).map_err(read_err)?;

    let manifest_text = manifest_text.ok_or_else(|| {
        ExtractManifestError::MissingManifest(PublishArchiveMissingManifestError {
            tarball_path: tarball_path.to_owned(),
        })
    })?;
    let mut manifest: Value = parse_manifest(&manifest_text)
        .map_err(|source| ExtractManifestError::Parse {
            tarball_path: tarball_path.to_owned(),
            source,
        })?;
    if let Some(readme) = preferred_readme {
        attach_readme(&mut manifest, readme);
    } else if let Some(candidate) = fallback_candidate
        && manifest_needs_readme(&manifest)
    {
        let file = archive.into_inner().into_inner();
        let readme = read_readme_candidate(file, candidate.index).map_err(read_err)?;
        attach_readme(&mut manifest, readme);
    }
    Ok(manifest)
}

struct PackedEntries {
    manifest_text: Option<String>,
    preferred_readme: Option<String>,
    fallback_candidate: Option<ReadmeCandidate>,
}

fn scan_packed_entries(archive: &mut tar::Archive<GzDecoder<File>>) -> io::Result<PackedEntries> {
    let mut manifest_text = None;
    let mut preferred_readme = None;
    let mut fallback_candidate = None;
    for (index, entry) in archive.entries()?.enumerate() {
        let mut entry = entry?;
        let normalized = normalize_entry_path(&entry.path()?);
        if normalized == "package/package.json" {
            manifest_text = Some(read_entry_text(&mut entry)?);
            continue;
        }
        if entry.header().entry_type().is_file()
            && let Some(readme_name) = root_readme_name(&normalized)
        {
            if readme_name.eq_ignore_ascii_case("readme.md") {
                preferred_readme = Some(read_entry_text(&mut entry)?);
            } else if preferred_readme.is_none() {
                select_readme_candidate(&mut fallback_candidate, readme_name, index);
            }
        }
    }
    Ok(PackedEntries { manifest_text, preferred_readme, fallback_candidate })
}

struct ReadmeCandidate {
    priority: u8,
    index: usize,
}

fn select_readme_candidate(candidate: &mut Option<ReadmeCandidate>, name: &str, index: usize) {
    let priority = if is_markdown_readme_file_name(name) { 2 } else { 1 };
    if candidate
        .as_ref()
        .is_none_or(|current| priority > current.priority)
    {
        *candidate = Some(ReadmeCandidate { priority, index });
    }
}

fn read_readme_candidate(mut file: File, index: usize) -> io::Result<String> {
    file.rewind()?;
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    let mut entries = archive.entries()?;
    let mut entry = entries
        .nth(index)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::UnexpectedEof, "selected README entry is missing")
        })??;
    read_entry_text(&mut entry)
}

fn read_entry_text<Reader: Read>(entry: &mut tar::Entry<'_, Reader>) -> io::Result<String> {
    let mut text = String::new();
    entry.read_to_string(&mut text)?;
    Ok(text)
}

/// A packed README fills a manifest's missing `readme`, as npm's publish
/// document carries it.
fn attach_readme(manifest: &mut Value, readme: String) {
    if manifest_needs_readme(manifest)
        && let Some(object) = manifest.as_object_mut()
    {
        object.insert("readme".to_string(), Value::String(readme));
    }
}

fn manifest_needs_readme(manifest: &Value) -> bool {
    manifest.get("readme").is_none_or(Value::is_null)
}

/// Return the package-root README filename when npm would recognize it.
fn root_readme_name(normalized: &str) -> Option<&str> {
    let name = normalized.strip_prefix("package/")?;
    (!name.contains('/') && is_readme_file_name(name)).then_some(name)
}

/// Normalize a tar entry path to forward slashes and collapse `.` / `..`
/// segments (so e.g. `package/./package.json` still matches).
///
/// `path.normalize` keeps what cannot be resolved: a leading `/` stays
/// (the result is still absolute) and a `..` with no real segment to pop is
/// preserved on a relative path. So `/package/package.json` and
/// `../package/package.json` normalize to themselves and must *not* match the
/// relative `package/package.json` the loop is looking for, exactly as in pnpm.
fn normalize_entry_path(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    if raw.is_empty() {
        return ".".to_owned();
    }
    let is_absolute = raw.starts_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in raw.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if matches!(segments.last(), Some(&last) if last != "..") {
                    segments.pop();
                } else if !is_absolute {
                    // A `..` cannot climb above the filesystem root, but on a
                    // relative path it climbs above the start, so keep it.
                    segments.push("..");
                }
            }
            other => segments.push(other),
        }
    }
    let joined = segments.join("/");
    match (is_absolute, joined.is_empty()) {
        (true, _) => format!("/{joined}"),
        (false, true) => ".".to_owned(),
        (false, false) => joined,
    }
}

/// Failure surface of [`extract_manifest_from_packed`].
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum ExtractManifestError {
    #[display("Failed to read the archive {tarball_path}: {source}")]
    #[diagnostic(code(ERR_PNPM_PUBLISH_EXTRACT_MANIFEST_READ))]
    Read {
        tarball_path: String,
        #[error(source)]
        source: std::io::Error,
    },

    #[display("Failed to parse package.json in {tarball_path}: {source}")]
    #[diagnostic(code(ERR_PNPM_PUBLISH_EXTRACT_MANIFEST_PARSE))]
    Parse {
        tarball_path: String,
        #[error(source)]
        source: serde_json::Error,
    },

    #[diagnostic(transparent)]
    MissingManifest(#[error(source)] PublishArchiveMissingManifestError),
}

/// The archive did not contain `package/package.json`
/// (`ERR_PNPM_PUBLISH_ARCHIVE_MISSING_MANIFEST`).
///
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
#[display("The archive {tarball_path} does not contain package/package.json")]
#[diagnostic(code(ERR_PNPM_PUBLISH_ARCHIVE_MISSING_MANIFEST))]
pub struct PublishArchiveMissingManifestError {
    pub tarball_path: String,
}

#[cfg(test)]
mod tests;
