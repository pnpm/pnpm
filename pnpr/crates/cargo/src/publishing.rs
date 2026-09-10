use super::{
    BTreeMap, CrateNameError, DependencyKind, Deserialize, Display, Error, IndexDependency,
    IndexEntry, MAX_CRATE_ARCHIVE_UNPACKED_BYTES, Read, Value, default_true, validate_crate_name,
};
use std::io;

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
    CrateName(CrateNameError),
    #[display("crate version {version:?} is not a semver version: {source}")]
    Version {
        version: String,
        source: semver::Error,
    },
    #[display("dependency {name:?} of the published crate: {source}")]
    DependencyName {
        name: String,
        source: CrateNameError,
    },
    #[display("dependency {name:?} has an invalid version requirement {req:?}: {source}")]
    DependencyRequirement {
        name: String,
        req: String,
        source: semver::Error,
    },
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

pub(super) fn take_length_prefixed<'body>(
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

pub(super) fn validate_crate_archive_with_limit(
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
        let Some(inner) = crate_entry_path(&entry, &expected)? else {
            continue;
        };
        if inner == "Cargo.toml" && entry.header().entry_type().is_file() {
            validate_crate_manifest(&mut entry, name, version)?;
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

/// One archive entry's path below the `<name>-<version>` root, or `None` for
/// the root directory itself. Anything that could escape the root, and any
/// entry that is neither a file nor a directory, is rejected.
pub(super) fn crate_entry_path<Reader: io::Read>(
    entry: &tar::Entry<'_, Reader>,
    expected: &str,
) -> Result<Option<String>, CrateArchiveError> {
    let path = entry.path().map_err(CrateArchiveError::Read)?;
    let path = path.to_string_lossy().into_owned();
    let entry_type = entry.header().entry_type();
    if path == expected && entry_type.is_dir() {
        return Ok(None);
    }
    let outside = || CrateArchiveError::EntryOutsideRoot {
        path: path.clone(),
        expected: expected.to_string(),
    };
    let Some(inner) = path.strip_prefix(expected).and_then(|rest| rest.strip_prefix('/')) else {
        return Err(outside());
    };
    if path.contains(['\\', ':']) || inner.split('/').any(|part| part == "..") {
        return Err(outside());
    }
    if !entry_type.is_file() && !entry_type.is_dir() {
        return Err(CrateArchiveError::UnsupportedEntry { path });
    }
    Ok(Some(inner.to_string()))
}

/// A crate's `Cargo.toml` must name the package the upload claims to be.
pub(super) fn validate_crate_manifest<Reader: io::Read>(
    entry: &mut tar::Entry<'_, Reader>,
    name: &str,
    version: &str,
) -> Result<(), CrateArchiveError> {
    let mut manifest = String::new();
    entry.read_to_string(&mut manifest).map_err(CrateArchiveError::Read)?;
    let matches = toml::from_str::<toml::Value>(&manifest).ok().is_some_and(|manifest| {
        let package = manifest.get("package");
        let field =
            |key| package.and_then(|package| package.get(key)).and_then(toml::Value::as_str);
        field("name") == Some(name) && field("version") == Some(version)
    });
    if matches {
        return Ok(());
    }
    Err(CrateArchiveError::InvalidManifest { name: name.to_string(), version: version.to_string() })
}
