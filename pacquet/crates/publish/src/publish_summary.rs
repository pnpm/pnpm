//! Port of [`tarball/publishSummary.ts`](https://github.com/pnpm/pnpm/blob/54c5c0e028/pnpm11/releasing/commands/src/tarball/publishSummary.ts): the per-package summary `pnpm publish`
//! returns and prints under `--json`, modeled after `npm publish --json`.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use ssri::{Algorithm, IntegrityOpts};

/// Per-package summary describing a successful publish. Ports TS
/// `PublishSummary`; field names serialize to the `npm publish --json` shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishSummary {
    /// Human-readable identifier `name@version`.
    pub id: String,
    pub name: String,
    pub version: String,
    /// Compressed tarball size in bytes.
    pub size: u64,
    /// Total uncompressed size of all files in the tarball, in bytes.
    pub unpacked_size: u64,
    /// Lowercase hex SHA-1 digest of the tarball.
    pub shasum: String,
    /// SRI-formatted SHA-512 digest of the tarball (`sha512-...`).
    pub integrity: String,
    /// Tarball file basename (e.g. `pkg-1.0.0.tgz`).
    pub filename: String,
    /// Files inside the tarball, in the `pnpm pack --json` shape.
    pub files: Vec<PublishSummaryFile>,
    /// Number of files inside the tarball.
    pub entry_count: usize,
    /// Names of bundled dependencies included in the tarball.
    pub bundled: Vec<String>,
    /// Staged-publish identifier; only present for staged publishes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_id: Option<String>,
}

/// One entry of [`PublishSummary::files`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublishSummaryFile {
    pub path: String,
}

/// The packed-package inputs to [`create_publish_summary`]. Ports TS
/// `PackedPkgInfo`.
pub struct PackedPkgInfo<'a> {
    pub published_manifest: &'a Value,
    pub tarball_path: &'a str,
    pub contents: &'a [String],
    pub unpacked_size: u64,
}

/// Build the [`PublishSummary`] for a freshly packed package. Ports TS
/// `createPublishSummary`.
#[must_use]
pub fn create_publish_summary(info: &PackedPkgInfo<'_>, tarball_data: &[u8]) -> PublishSummary {
    let name = manifest_string(info.published_manifest, "name");
    let version = manifest_string(info.published_manifest, "version");
    let filename = Path::new(info.tarball_path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    PublishSummary {
        id: format!("{name}@{version}"),
        name,
        version,
        size: tarball_data.len() as u64,
        unpacked_size: info.unpacked_size,
        // SHA-1 is the legacy `dist.shasum`; `integrity` is the modern SRI hash.
        shasum: sha1_hex(tarball_data),
        integrity: sha512_sri(tarball_data),
        filename,
        files: info.contents.iter().map(|path| PublishSummaryFile { path: path.clone() }).collect(),
        entry_count: info.contents.len(),
        bundled: extract_bundled_dependencies(info.published_manifest),
        stage_id: None,
    }
}

/// Normalize the two equivalent manifest keys (`bundledDependencies` and
/// `bundleDependencies`) into a flat list of dependency names, matching npm's
/// interpretation. Ports TS `extractBundledDependencies`.
#[must_use]
pub fn extract_bundled_dependencies(manifest: &Value) -> Vec<String> {
    let raw = manifest.get("bundledDependencies").or_else(|| manifest.get("bundleDependencies"));
    match raw {
        Some(Value::Array(items)) => {
            items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect()
        }
        // `true` means "bundle every dependency"; expand it to the names.
        Some(Value::Bool(true)) => manifest
            .get("dependencies")
            .and_then(Value::as_object)
            .map(|deps| deps.keys().cloned().collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn manifest_string(manifest: &Value, key: &str) -> String {
    manifest.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn sha1_hex(data: &[u8]) -> String {
    let mut opts = IntegrityOpts::new().algorithm(Algorithm::Sha1);
    opts.input(data);
    opts.result().to_hex().1
}

fn sha512_sri(data: &[u8]) -> String {
    let mut opts = IntegrityOpts::new().algorithm(Algorithm::Sha512);
    opts.input(data);
    opts.result().to_string()
}

#[cfg(test)]
mod tests;
