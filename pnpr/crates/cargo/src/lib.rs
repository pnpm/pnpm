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

pub use publishing::{
    CrateArchiveError, PublishBodyError, PublishDependency, PublishMetadata, PublishMetadataError,
    parse_publish_body, validate_crate_archive,
};

pub use pnpr_package_name::{CrateNameError, MAX_CRATE_NAME_LEN};

mod publishing;

use derive_more::{Display, Error};
use pnpr_package_name::canonicalize_crate_name;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io::Read};

/// The URL segment under a registry endpoint where the sparse index lives:
/// `/~<name>/index/config.json`, `/~<name>/index/se/rd/serde`.
pub const INDEX_PATH: &str = "index";

/// The URL segment under a registry endpoint where the crates web API lives:
/// `/~<name>/api/v1/crates/new`, `/~<name>/api/v1/crates/<crate>/<version>/download`.
pub const API_PATH: &str = "api/v1/crates";

/// The decompressed size at which a published crate archive is rejected: a
/// bound on the work a gzip bomb can force onto the archive check.
pub const MAX_CRATE_ARCHIVE_UNPACKED_BYTES: u64 = 512 * 1024 * 1024;

/// Validate a crate name the way crates.io does: ASCII letters, digits, `-`
/// and `_`, starting with a letter or `_`, at most 64 characters. Every
/// crate name in a URL, a publish body, or an index entry passes through
/// here before it is used as a storage path segment.
pub fn validate_crate_name(name: &str) -> Result<(), CrateNameError> {
    canonicalize_crate_name(name).map(|_| ())
}

/// The directory part of a crate's sparse-index path, in the name's own
/// case: `1`, `2`, `3/<first letter>`, or `<first two>/<next two>`.
/// `cargo` requests the lowercase form ([`sparse_index_path`]); the cased
/// form is what the `{prefix}` download-template marker expands to.
#[must_use]
pub fn index_prefix(name: &str) -> String {
    pnpm_cargo_resolver::index_prefix(name)
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
    let numbered = text.lines().enumerate().filter(|(_, line)| !line.trim().is_empty());
    numbered
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
    /// The description of the most recently published version, which is
    /// what the crates API reports for the crate. The sparse index carries
    /// no description, so this is the only place a hosted crate has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl CrateDocument {
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), versions: Vec::new(), description: None }
    }

    /// The version the crates API reports as `max_version`: the highest
    /// non-yanked release, or the highest of all of them once every release
    /// is yanked. `None` for a document with no parsable version.
    #[must_use]
    pub fn max_version(&self) -> Option<String> {
        let highest = |yanked_allowed: bool| {
            self.versions
                .iter()
                .filter(|entry| yanked_allowed || !entry.yanked)
                .filter_map(|entry| semver::Version::parse(&entry.vers).ok())
                .max()
        };
        highest(false).or_else(|| highest(true)).map(|version| version.to_string())
    }

    /// This crate as one row of a search response.
    #[must_use]
    pub fn to_search_crate(&self) -> SearchCrate {
        SearchCrate {
            name: self.name.clone(),
            description: self.description.clone(),
            max_version: self.max_version().unwrap_or_default(),
        }
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

/// The longest description a crate document keeps, which is also the cap
/// crates.io puts on one. A search response carries a page of descriptions,
/// and a publisher writes them, so an unbounded one would let a publisher
/// decide how large every later search response is.
pub const MAX_DESCRIPTION_LEN: usize = 1_000;

/// A publish's description, cut to [`MAX_DESCRIPTION_LEN`] characters. Cut
/// rather than refused: the description was accepted and discarded before
/// crate documents kept one, and a publish that worked should keep working.
#[must_use]
pub fn bounded_description(description: Option<&str>) -> Option<String> {
    description.map(|description| description.chars().take(MAX_DESCRIPTION_LEN).collect())
}

/// One row of `GET api/v1/crates`. `cargo search` reads exactly these three
/// fields, so the response stays that narrow rather than modelling all of
/// what crates.io returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchCrate {
    pub name: String,
    pub description: Option<String>,
    pub max_version: String,
}

/// The body of `GET api/v1/crates`. `total` counts every match, not the
/// page, so a client can tell that a query was truncated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub crates: Vec<SearchCrate>,
    pub meta: SearchMeta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchMeta {
    pub total: usize,
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
    pnpm_cargo_resolver::download_url(template, name, version, cksum)
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
