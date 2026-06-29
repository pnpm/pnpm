//! Port of [`extractManifestFromPacked.ts`](https://github.com/pnpm/pnpm/blob/54c5c0e028/pnpm11/releasing/commands/src/publish/extractManifestFromPacked.ts): read `package/package.json` out of
//! a pre-built `.tgz` so a tarball passed to `pnpm publish <tarball>` can be
//! published without repacking.

use std::{fs::File, io::Read, path::Path};

use flate2::read::GzDecoder;
use pacquet_diagnostics::miette::{self, Diagnostic};
use serde_json::Value;

const TARBALL_SUFFIXES: [&str; 2] = [".tar.gz", ".tgz"];

/// Whether `path` looks like a publishable tarball (ends with `.tar.gz` or
/// `.tgz`). Ports TS `isTarballPath`.
#[must_use]
pub fn is_tarball_path(path: &str) -> bool {
    TARBALL_SUFFIXES.iter().any(|suffix| path.ends_with(suffix))
}

/// Read and parse `package/package.json` from the gzipped tarball at
/// `tarball_path`. Ports TS `extractManifestFromPacked`.
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
        return serde_json::from_str(&text).map_err(|source| ExtractManifestError::Parse {
            tarball_path: tarball_path.to_owned(),
            source,
        });
    }

    Err(ExtractManifestError::MissingManifest(PublishArchiveMissingManifestError {
        tarball_path: tarball_path.to_owned(),
    }))
}

/// Normalize a tar entry path to forward slashes and collapse `.` / `..`
/// segments, mirroring the TS `path.normalize(name).replaceAll('\\', '/')`
/// comparison (so e.g. `package/./package.json` still matches).
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
    #[diagnostic(code(pacquet_publish::extract_manifest::read))]
    Read {
        tarball_path: String,
        #[error(source)]
        source: std::io::Error,
    },

    #[display("Failed to parse package.json in {tarball_path}: {source}")]
    #[diagnostic(code(pacquet_publish::extract_manifest::parse))]
    Parse {
        tarball_path: String,
        #[error(source)]
        source: serde_json::Error,
    },

    #[diagnostic(transparent)]
    MissingManifest(#[error(source)] PublishArchiveMissingManifestError),
}

/// The archive did not contain `package/package.json`. Ports pnpm's
/// `PublishArchiveMissingManifestError` (`ERR_PNPM_PUBLISH_ARCHIVE_MISSING_MANIFEST`).
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
#[display("The archive {tarball_path} does not contain package/package.json")]
#[diagnostic(code(ERR_PNPM_PUBLISH_ARCHIVE_MISSING_MANIFEST))]
pub struct PublishArchiveMissingManifestError {
    pub tarball_path: String,
}

#[cfg(test)]
mod tests;
