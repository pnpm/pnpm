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
    let file = File::open(tarball_path).map_err(|source| ExtractManifestError::Read {
        tarball_path: tarball_path.to_owned(),
        source,
    })?;
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    let entries = archive.entries().map_err(|source| ExtractManifestError::Read {
        tarball_path: tarball_path.to_owned(),
        source,
    })?;

    for entry in entries {
        let mut entry = entry.map_err(|source| ExtractManifestError::Read {
            tarball_path: tarball_path.to_owned(),
            source,
        })?;
        let path = entry.path().map_err(|source| ExtractManifestError::Read {
            tarball_path: tarball_path.to_owned(),
            source,
        })?;
        if normalize_entry_path(&path) != "package/package.json" {
            continue;
        }
        let mut text = String::new();
        entry.read_to_string(&mut text).map_err(|source| ExtractManifestError::Read {
            tarball_path: tarball_path.to_owned(),
            source,
        })?;
        return serde_json::from_str(&text).map_err(|source| ExtractManifestError::Parse {
            tarball_path: tarball_path.to_owned(),
            source,
        });
    }

    Err(ExtractManifestError::MissingManifest(PublishArchiveMissingManifestError {
        tarball_path: tarball_path.to_owned(),
    }))
}

/// Normalize a tar entry path to forward slashes and drop a leading `./`,
/// mirroring the TS `path.normalize(name).replaceAll('\\', '/')` comparison.
fn normalize_entry_path(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    normalized.strip_prefix("./").unwrap_or(&normalized).to_owned()
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
