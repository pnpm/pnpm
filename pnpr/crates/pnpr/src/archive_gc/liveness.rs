//! Which of a package's stored archives its metadata still references, by
//! the rules of its ecosystem.
//!
//! An archive is collectable only when its file name is one the ecosystem's
//! publishes produce and nothing live in the package's document reaches it.
//! A file name the rules do not recognize is never collectable: it may be
//! something a newer pnpr, or an operator, put there.

use crate::{RegistryError, Result};
use pnpr_cargo::CrateDocument;
use pnpr_oci::{Descriptor, Digest, ImageDocument, Manifest, media_type};
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use pnpr_pypi::ProjectDocument;
use pnpr_storage::Storage;
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};

/// What a package's document keeps alive.
pub(crate) enum Live {
    /// npm and Cargo: the versions the document holds. Every archive of one
    /// of them stays.
    Versions(BTreeSet<String>),
    /// `PyPI`: the files the document lists.
    Files(BTreeSet<String>),
    /// OCI: every blob a retained manifest reaches.
    Blobs(HashSet<String>),
}

impl Live {
    /// What the document `bytes` of `name` keeps alive, `None` when the
    /// package has no metadata. `storage` is the package's namespace, from
    /// which an image's manifests are read back.
    pub(crate) async fn of(
        storage: &Storage,
        name: &CanonicalPackageName,
        bytes: Option<&[u8]>,
        manifest_limit: usize,
    ) -> Result<Self> {
        let ecosystem = name.ecosystem();
        let Some(bytes) = bytes else {
            return Ok(match ecosystem {
                Ecosystem::Npm | Ecosystem::Cargo => Self::Versions(BTreeSet::new()),
                Ecosystem::Pypi => Self::Files(BTreeSet::new()),
                Ecosystem::Oci => Self::Blobs(HashSet::new()),
            });
        };
        Ok(match ecosystem {
            Ecosystem::Npm => {
                let document: Value = serde_json::from_slice(bytes)?;
                let versions = document
                    .get("versions")
                    .and_then(Value::as_object)
                    .map(|versions| versions.keys().cloned().collect())
                    .unwrap_or_default();
                Self::Versions(versions)
            }
            Ecosystem::Cargo => {
                let document: CrateDocument = serde_json::from_slice(bytes)?;
                Self::Versions(
                    document.versions
                        .into_iter()
                        .map(|entry| entry.vers)
                        .collect(),
                )
            }
            Ecosystem::Pypi => {
                let document: ProjectDocument = serde_json::from_slice(bytes)?;
                Self::Files(
                    document.files
                        .into_iter()
                        .map(|file| file.filename)
                        .collect(),
                )
            }
            Ecosystem::Oci => {
                let document = parse_image_document(bytes)?;
                Self::Blobs(
                    referenced_document_blobs(storage, name, &document, manifest_limit).await?,
                )
            }
        })
    }

    /// Whether `filename` of `name` is an archive this document no longer
    /// references. A name the ecosystem's publishes do not produce is not.
    pub(crate) fn is_unreferenced(&self, name: &CanonicalPackageName, filename: &str) -> bool {
        match self {
            Self::Versions(versions) => {
                archive_version(name, filename).is_some_and(|version| !versions.contains(&version))
            }
            Self::Files(files) => {
                pnpr_pypi::parse_distribution_filename(filename).is_ok()
                    && !files.contains(filename)
            }
            Self::Blobs(blobs) => filename_digest(filename).is_some() && !blobs.contains(filename),
        }
    }
}

/// The version an archive of `name` belongs to, by its file name, when the
/// name is one the ecosystem's publishes produce: for npm a `.tgz`, a
/// variant's `<version>~<tag>.tgz`, a stored `.tar.zst`, or a `.sigstore`
/// bundle; for Cargo a `.crate`; for `PyPI` a wheel or sdist. `None` for an
/// OCI blob, which belongs to no version.
pub(crate) fn archive_version(name: &CanonicalPackageName, filename: &str) -> Option<String> {
    match name.ecosystem() {
        Ecosystem::Npm => {
            let stem = [".tar.zst", ".sigstore", ".tgz"]
                .iter()
                .find_map(|suffix| filename.strip_suffix(suffix))?;
            let (_, version) = name
                .parse_tarball_name(&format!("{stem}.tgz"))
                .ok()?;
            // A variant's tag follows the version after a `~`, which no
            // version holds.
            let version = version.split('~').next()?;
            node_semver::Version::parse(version).ok()?;
            Some(version.to_string())
        }
        Ecosystem::Cargo => {
            let stem = filename.strip_suffix(".crate")?;
            let prefix = stem.get(..name.as_str().len())?;
            let version = stem
                .get(name.as_str().len()..)?
                .strip_prefix('-')?;
            (prefix.eq_ignore_ascii_case(name.as_str())
                && node_semver::Version::parse(version).is_ok())
            .then(|| version.to_string())
        }
        Ecosystem::Pypi => pnpr_pypi::parse_distribution_filename(filename)
            .ok()
            .map(|distribution| distribution.version),
        Ecosystem::Oci => None,
    }
}

/// The digest an OCI blob is stored under its file name for.
pub(crate) fn filename_digest(filename: &str) -> Option<Digest> {
    Digest::parse(&format!("sha256:{}", filename.strip_prefix("sha256-")?)).ok()
}

pub(crate) fn parse_image_document(body: &[u8]) -> Result<ImageDocument> {
    let document = ImageDocument::parse(body)?;
    if document.format_version > pnpr_oci::IMAGE_DOCUMENT_FORMAT_VERSION {
        return Err(RegistryError::DocumentFormatTooNew {
            document: "image document".to_string(),
            format_version: document.format_version,
            supported_version: pnpr_oci::IMAGE_DOCUMENT_FORMAT_VERSION,
        });
    }
    Ok(document)
}

/// Every blob a manifest `document` retains reaches, the manifests
/// included. A retained manifest that is missing or corrupt is an error,
/// so nothing it might have reached is collected.
pub(crate) async fn referenced_document_blobs(
    storage: &Storage,
    name: &CanonicalPackageName,
    document: &ImageDocument,
    manifest_limit: usize,
) -> Result<HashSet<String>> {
    let mut pending: Vec<(Digest, Option<String>)> = document
        .manifests()
        .iter()
        .map(|entry| (entry.digest.clone(), Some(entry.media_type.clone())))
        .collect();
    let mut visited = HashSet::new();
    let mut reachable = HashSet::new();
    while let Some((digest, content_type)) = pending.pop() {
        if !visited.insert(digest.clone()) {
            continue;
        }
        let filename = digest.blob_filename();
        reachable.insert(filename.clone());
        let manifest =
            read_manifest_blob(storage, name, &digest, content_type.as_deref(), manifest_limit)
                .await?;
        for reference in manifest.references() {
            reachable.insert(reference.digest.blob_filename());
            if media_type::is_index(manifest.media_type()) {
                let content_type = resolve_reference_media_type(reference, document);
                pending.push((reference.digest.clone(), content_type));
            }
        }
    }
    Ok(reachable)
}

fn resolve_reference_media_type(
    reference: &Descriptor,
    document: &ImageDocument,
) -> Option<String> {
    reference.media_type
        .clone()
        .or_else(|| {
            document.manifest(&reference.digest).map(|entry| entry.media_type.clone())
        })
}

/// Read one retained manifest back and parse it, checking it is the blob its
/// digest names.
async fn read_manifest_blob(
    storage: &Storage,
    name: &CanonicalPackageName,
    digest: &Digest,
    content_type: Option<&str>,
    manifest_limit: usize,
) -> Result<Manifest> {
    let filename = digest.blob_filename();
    let invalid = |reason: String| RegistryError::BadRequest {
        reason: format!("cannot collect {}/{filename}: {reason}", name.as_str()),
    };
    let (body, _) = storage
        .open_hosted_blob(name, &filename)
        .await?
        .ok_or_else(|| invalid("retained manifest is missing".into()))?;
    let bytes = axum::body::to_bytes(body, manifest_limit).await
        .map_err(|error| invalid(error.to_string()))?;
    if Digest::of(&bytes) != *digest {
        return Err(invalid("manifest digest mismatch".into()));
    }
    Manifest::parse(&bytes, content_type).map_err(|error| invalid(error.to_string()))
}
