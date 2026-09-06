//! The Cargo registry protocol pnpr speaks.
//!
//! A Cargo registry is two HTTP surfaces: a **sparse index** (a `config.json`
//! plus one newline-delimited JSON file per crate, laid out by name prefix)
//! that `cargo` reads to resolve, and a **web API** it downloads crates from
//! and publishes to. This crate holds the protocol pieces the server needs
//! and nothing about storage or routing: the index path layout, the index
//! entry format, the `cargo publish` request body, the `config.json`
//! document with its download URL template, and the crate-archive checks a
//! publish runs before accepting bytes.

use derive_more::{Display, Error};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, Read},
};

/// The URL segment under a registry endpoint where the sparse index lives:
/// `/~<name>/index/config.json`, `/~<name>/index/se/rd/serde`.
pub const INDEX_PATH: &str = "index";

/// The URL segment under a registry endpoint where the crates web API lives:
/// `/~<name>/api/v1/crates/new`, `/~<name>/api/v1/crates/<crate>/<version>/download`.
pub const API_PATH: &str = "api/v1/crates";

/// The longest crate name crates.io accepts.
pub const MAX_CRATE_NAME_LEN: usize = 64;

/// The decompressed size at which a published crate archive is rejected: a
/// bound on the work a gzip bomb can force onto the archive check.
pub const MAX_CRATE_ARCHIVE_UNPACKED_BYTES: u64 = 512 * 1024 * 1024;

/// A crate name that no Cargo registry can serve.
#[derive(Debug, Display, Error, Clone, PartialEq, Eq)]
pub enum CrateNameError {
    #[display("crate name must not be empty")]
    Empty,
    #[display("crate name {name:?} is longer than {MAX_CRATE_NAME_LEN} characters")]
    TooLong { name: String },
    #[display("crate name {name:?} must start with a letter or `_`")]
    InvalidStart { name: String },
    #[display("crate name {name:?} may only contain ASCII letters, digits, `-` and `_`")]
    InvalidCharacter { name: String },
}

/// Validate a crate name the way crates.io does: ASCII letters, digits, `-`
/// and `_`, starting with a letter or `_`, at most 64 characters. Every
/// crate name in a URL, a publish body, or an index entry passes through
/// here before it is used as a storage path segment.
pub fn validate_crate_name(name: &str) -> Result<(), CrateNameError> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(CrateNameError::Empty);
    };
    if name.len() > MAX_CRATE_NAME_LEN {
        return Err(CrateNameError::TooLong { name: name.to_string() });
    }
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(CrateNameError::InvalidStart { name: name.to_string() });
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')) {
        return Err(CrateNameError::InvalidCharacter { name: name.to_string() });
    }
    Ok(())
}

/// The directory part of a crate's sparse-index path, in the name's own
/// case: `1`, `2`, `3/<first letter>`, or `<first two>/<next two>`.
/// `cargo` requests the lowercase form ([`sparse_index_path`]); the cased
/// form is what the `{prefix}` download-template marker expands to.
#[must_use]
pub fn index_prefix(name: &str) -> String {
    match name.len() {
        1 => "1".to_string(),
        2 => "2".to_string(),
        3 => format!("3/{}", &name[..1]),
        _ => format!("{}/{}", &name[..2], &name[2..4]),
    }
}

/// The relative path of a crate's file inside a sparse index, lowercased as
/// `cargo` requests it. The name must already have passed
/// [`validate_crate_name`].
#[must_use]
pub fn sparse_index_path(name: &str) -> String {
    let lowercase = name.to_ascii_lowercase();
    format!("{}/{lowercase}", index_prefix(&lowercase))
}

#[must_use]
pub fn crate_filename(name: &str, version: &str) -> String {
    format!("{name}-{version}.crate")
}

/// The kind of a dependency edge, as spelled in both the publish body and
/// the index (`normal`, `build`, `dev`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyKind {
    #[default]
    Normal,
    Build,
    Dev,
}

/// One dependency of an index entry, in the wire shape `cargo` reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexDependency {
    /// The name the depending crate refers to the dependency by: the
    /// renamed alias when the dependency is renamed, else the package name.
    pub name: String,
    pub req: String,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default = "default_true")]
    pub default_features: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default)]
    pub kind: DependencyKind,
    /// The index URL of the registry the dependency comes from; `None` for
    /// the registry this entry lives in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
    /// The real package name when `name` is a rename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
}

/// One line of a sparse-index file: one published version of a crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub name: String,
    pub vers: String,
    #[serde(default)]
    pub deps: Vec<IndexDependency>,
    /// Lowercase hex SHA-256 of the `.crate` archive.
    pub cksum: String,
    #[serde(default)]
    pub features: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub yanked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<String>,
    /// The index schema version of this entry. `1` is the original format;
    /// `2` adds `features2`, which older `cargo` versions must not see in
    /// `features`.
    #[serde(default = "default_index_schema_version")]
    pub v: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features2: Option<BTreeMap<String, Vec<String>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rust_version: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_index_schema_version() -> u32 {
    1
}

/// A sparse-index file that could not be read as index entries.
#[derive(Debug, Display, Error)]
#[display("sparse index line {line} is not an index entry: {source}")]
pub struct IndexParseError {
    pub line: usize,
    pub source: serde_json::Error,
}

pub fn parse_index(text: &str) -> Result<Vec<IndexEntry>, IndexParseError> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str(line).map_err(|source| IndexParseError { line: index + 1, source })
        })
        .collect()
}

#[must_use]
pub fn render_index(entries: &[IndexEntry]) -> String {
    let mut text = String::new();
    for entry in entries {
        text.push_str(&serde_json::to_string(entry).expect("index entry serializes"));
        text.push('\n');
    }
    text
}

/// The hosted document pnpr stores per crate: the crate's published
/// versions as index entries. The sparse-index file is rendered from it on
/// read, and yank flips an entry in place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrateDocument {
    pub name: String,
    pub versions: Vec<IndexEntry>,
}

impl CrateDocument {
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), versions: Vec::new() }
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("crate document serializes")
    }

    #[must_use]
    pub fn version(&self, vers: &str) -> Option<&IndexEntry> {
        self.versions.iter().find(|entry| entry.vers == vers)
    }

    pub fn version_mut(&mut self, vers: &str) -> Option<&mut IndexEntry> {
        self.versions.iter_mut().find(|entry| entry.vers == vers)
    }

    #[must_use]
    pub fn render_index(&self) -> String {
        render_index(&self.versions)
    }
}

/// The `config.json` at the root of a sparse index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexConfig {
    /// The download URL template; see [`download_url`].
    pub dl: String,
    /// The web API base URL, used by `cargo publish`, `yank`, `search`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    /// When set, `cargo` sends its token on index and download requests as
    /// well as on API calls.
    #[serde(rename = "auth-required", default, skip_serializing_if = "std::ops::Not::not")]
    pub auth_required: bool,
}

impl IndexConfig {
    /// The `config.json` a pnpr registry endpoint at `base` (no trailing
    /// slash) advertises: downloads and the API both point back at it.
    #[must_use]
    pub fn for_registry(base: &str, auth_required: bool) -> Self {
        Self { dl: format!("{base}/{API_PATH}"), api: Some(base.to_string()), auth_required }
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

/// Expand a `config.json` `dl` template for one crate version. The markers
/// are `{crate}`, `{version}`, `{prefix}`, `{lowerprefix}` and
/// `{sha256-checksum}`; a template with none of them gets
/// `/{crate}/{version}/download` appended, as `cargo` does.
#[must_use]
pub fn download_url(template: &str, name: &str, version: &str, cksum: &str) -> String {
    const MARKERS: [&str; 5] =
        ["{crate}", "{version}", "{prefix}", "{lowerprefix}", "{sha256-checksum}"];
    if !MARKERS.iter().any(|marker| template.contains(marker)) {
        return format!("{}/{name}/{version}/download", template.trim_end_matches('/'));
    }
    template
        .replace("{crate}", name)
        .replace("{version}", version)
        .replace("{prefix}", &index_prefix(name))
        .replace("{lowerprefix}", &index_prefix(&name.to_ascii_lowercase()))
        .replace("{sha256-checksum}", cksum)
}

/// A `cargo publish` request body that could not be read.
#[derive(Debug, Display, Error)]
pub enum PublishBodyError {
    #[display("publish body is truncated: expected {expected} more bytes")]
    Truncated { expected: usize },
    #[display(
        "publish body declares a {field} of {declared} bytes, more than the {remaining} left"
    )]
    LengthOverrun { field: &'static str, declared: usize, remaining: usize },
    #[display("publish body has {trailing} trailing bytes after the crate archive")]
    TrailingBytes { trailing: usize },
    #[display("publish metadata is not valid JSON: {_0}")]
    Metadata(serde_json::Error),
}

/// One dependency as `cargo publish` sends it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PublishDependency {
    /// The dependency's package name.
    pub name: String,
    pub version_req: String,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default = "default_true")]
    pub default_features: bool,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub kind: DependencyKind,
    #[serde(default)]
    pub registry: Option<String>,
    /// The alias the depending crate uses when it renames the dependency.
    #[serde(default)]
    pub explicit_name_in_toml: Option<String>,
}

/// The JSON metadata half of a `cargo publish` body. Fields that only feed
/// a registry's web UI (description, license, links, badges, ...) are
/// accepted and retained but do not reach the index.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PublishMetadata {
    pub name: String,
    pub vers: String,
    #[serde(default)]
    pub deps: Vec<PublishDependency>,
    #[serde(default)]
    pub features: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub documentation: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub readme: Option<String>,
    #[serde(default)]
    pub readme_file: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub license_file: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub badges: Value,
    #[serde(default)]
    pub links: Option<String>,
    #[serde(default)]
    pub rust_version: Option<String>,
}

/// Publish metadata a registry must refuse.
#[derive(Debug, Display, Error)]
pub enum PublishMetadataError {
    #[display("{_0}")]
    CrateName(CrateNameError),
    #[display("crate version {version:?} is not a semver version: {source}")]
    Version { version: String, source: semver::Error },
    #[display("dependency {name:?} of the published crate: {source}")]
    DependencyName { name: String, source: CrateNameError },
    #[display("dependency {name:?} has an invalid version requirement {req:?}: {source}")]
    DependencyRequirement { name: String, req: String, source: semver::Error },
}

impl PublishMetadata {
    /// Reject metadata whose names or versions no registry could index.
    pub fn validate(&self) -> Result<(), PublishMetadataError> {
        validate_crate_name(&self.name).map_err(PublishMetadataError::CrateName)?;
        semver::Version::parse(&self.vers).map_err(|source| PublishMetadataError::Version {
            version: self.vers.clone(),
            source,
        })?;
        for dep in &self.deps {
            validate_crate_name(&dep.name).map_err(|source| {
                PublishMetadataError::DependencyName { name: dep.name.clone(), source }
            })?;
            if let Some(alias) = &dep.explicit_name_in_toml {
                validate_crate_name(alias).map_err(|source| {
                    PublishMetadataError::DependencyName { name: alias.clone(), source }
                })?;
            }
            semver::VersionReq::parse(&dep.version_req).map_err(|source| {
                PublishMetadataError::DependencyRequirement {
                    name: dep.name.clone(),
                    req: dep.version_req.clone(),
                    source,
                }
            })?;
        }
        Ok(())
    }

    /// The index entry this publish adds, given the archive's SHA-256.
    /// Features written in the `dep:` / weak-dependency syntax go to
    /// `features2` under schema version 2, where a `cargo` too old to
    /// understand them does not read them.
    #[must_use]
    pub fn into_index_entry(self, cksum: String) -> IndexEntry {
        let deps = self
            .deps
            .into_iter()
            .map(|dep| {
                let (name, package) = match dep.explicit_name_in_toml {
                    Some(alias) => (alias, Some(dep.name)),
                    None => (dep.name, None),
                };
                IndexDependency {
                    name,
                    req: dep.version_req,
                    features: dep.features,
                    optional: dep.optional,
                    default_features: dep.default_features,
                    target: dep.target,
                    kind: dep.kind,
                    registry: dep.registry,
                    package,
                }
            })
            .collect();
        let (features, features2): (BTreeMap<_, _>, BTreeMap<_, _>) =
            self.features.into_iter().partition(|(_, values)| {
                !values.iter().any(|value| value.starts_with("dep:") || value.contains("?/"))
            });
        let schema_version = if features2.is_empty() { 1 } else { 2 };
        IndexEntry {
            name: self.name,
            vers: self.vers,
            deps,
            cksum,
            features,
            yanked: false,
            links: self.links,
            v: schema_version,
            features2: (!features2.is_empty()).then_some(features2),
            rust_version: self.rust_version,
        }
    }
}

/// Split a `cargo publish` body into its metadata and the crate archive
/// bytes. The body is a 32-bit little-endian length, that many bytes of
/// JSON metadata, another length, and that many bytes of `.crate` archive.
pub fn parse_publish_body(body: &[u8]) -> Result<(PublishMetadata, &[u8]), PublishBodyError> {
    let (metadata, rest) = take_length_prefixed(body, "metadata")?;
    let (archive, rest) = take_length_prefixed(rest, "crate archive")?;
    if !rest.is_empty() {
        return Err(PublishBodyError::TrailingBytes { trailing: rest.len() });
    }
    let metadata = serde_json::from_slice(metadata).map_err(PublishBodyError::Metadata)?;
    Ok((metadata, archive))
}

fn take_length_prefixed<'body>(
    body: &'body [u8],
    field: &'static str,
) -> Result<(&'body [u8], &'body [u8]), PublishBodyError> {
    let Some((length, rest)) = body.split_first_chunk::<4>() else {
        return Err(PublishBodyError::Truncated { expected: 4 - body.len() });
    };
    let declared = u32::from_le_bytes(*length) as usize;
    if declared > rest.len() {
        return Err(PublishBodyError::LengthOverrun { field, declared, remaining: rest.len() });
    }
    Ok(rest.split_at(declared))
}

/// A published archive that is not the crate it claims to be.
#[derive(Debug, Display, Error)]
pub enum CrateArchiveError {
    #[display("crate archive is not a gzip-compressed tar archive: {_0}")]
    Read(io::Error),
    #[display("crate archive entry {path:?} is outside the {expected:?} directory")]
    EntryOutsideRoot { path: String, expected: String },
    #[display("crate archive has no {expected}/Cargo.toml")]
    MissingManifest { expected: String },
    #[display("crate archive unpacks to more than {MAX_CRATE_ARCHIVE_UNPACKED_BYTES} bytes")]
    TooLarge,
    #[display("crate archive entry {path:?} is not a regular file or directory")]
    UnsupportedEntry { path: String },
    #[display("crate archive Cargo.toml does not declare package {name:?} version {version:?}")]
    InvalidManifest { name: String, version: String },
}

/// Accepts gzip-compressed tar archives containing only regular files and
/// directories within `<name>-<version>`, with a matching `Cargo.toml`.
/// Decompressed size must not exceed [`MAX_CRATE_ARCHIVE_UNPACKED_BYTES`].
pub fn validate_crate_archive(
    archive: &[u8],
    name: &str,
    version: &str,
) -> Result<(), CrateArchiveError> {
    validate_crate_archive_with_limit(archive, name, version, MAX_CRATE_ARCHIVE_UNPACKED_BYTES)
}

fn validate_crate_archive_with_limit(
    archive: &[u8],
    name: &str,
    version: &str,
    limit: u64,
) -> Result<(), CrateArchiveError> {
    let expected = format!("{name}-{version}");
    let decoder = flate2::read::MultiGzDecoder::new(archive);
    let mut limited = decoder.take(limit + 1);
    let mut tar = tar::Archive::new(&mut limited);
    let mut found_manifest = false;
    let entries = tar.entries().map_err(CrateArchiveError::Read)?;
    for entry in entries {
        let mut entry = entry.map_err(CrateArchiveError::Read)?;
        let path = entry.path().map_err(CrateArchiveError::Read)?;
        let path = path.to_string_lossy().into_owned();
        let entry_type = entry.header().entry_type();
        if path == expected && entry_type.is_dir() {
            continue;
        }
        let Some(inner) = path.strip_prefix(&expected).and_then(|rest| rest.strip_prefix('/'))
        else {
            return Err(CrateArchiveError::EntryOutsideRoot { path, expected });
        };
        if path.contains(['\\', ':']) || inner.split('/').any(|part| part == "..") {
            return Err(CrateArchiveError::EntryOutsideRoot { path, expected });
        }
        if !entry_type.is_file() && !entry_type.is_dir() {
            return Err(CrateArchiveError::UnsupportedEntry { path });
        }
        if inner == "Cargo.toml" && entry_type.is_file() {
            let mut manifest = String::new();
            entry.read_to_string(&mut manifest).map_err(CrateArchiveError::Read)?;
            let matches = toml::from_str::<toml::Value>(&manifest).ok().is_some_and(|manifest| {
                let package = manifest.get("package");
                package.and_then(|package| package.get("name")).and_then(toml::Value::as_str)
                    == Some(name)
                    && package
                        .and_then(|package| package.get("version"))
                        .and_then(toml::Value::as_str)
                        == Some(version)
            });
            if !matches {
                return Err(CrateArchiveError::InvalidManifest {
                    name: name.to_string(),
                    version: version.to_string(),
                });
            }
            found_manifest = true;
        }
    }
    io::copy(&mut limited, &mut io::sink()).map_err(CrateArchiveError::Read)?;
    if limited.limit() == 0 {
        return Err(CrateArchiveError::TooLarge);
    }
    if !found_manifest {
        return Err(CrateArchiveError::MissingManifest { expected });
    }
    Ok(())
}

#[must_use]
pub fn errors_json(detail: &str) -> Value {
    json!({ "errors": [{ "detail": detail }] })
}

#[must_use]
pub fn publish_ok_json() -> Value {
    json!({ "warnings": { "invalid_categories": [], "invalid_badges": [], "other": [] } })
}

#[must_use]
pub fn ok_json() -> Value {
    json!({ "ok": true })
}

#[cfg(test)]
mod tests;
