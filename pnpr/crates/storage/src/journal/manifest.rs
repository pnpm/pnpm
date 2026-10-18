use super::{Ecosystem, JournaledRevisionRef, PackageId, default_format_version};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Manifest {
    #[serde(rename = "formatVersion", default = "default_format_version")]
    pub(super) format_version: u32,
    pub(super) packages: Vec<ManifestPackage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct ManifestPackage {
    pub(super) name: String,
    /// The ecosystem whose document format this package's document is in.
    /// An entry that names none holds an npm packument.
    #[serde(default)]
    pub(super) ecosystem: Ecosystem,
    /// Hosted-org storage namespace this package publishes into, or `None` for
    /// the flat (path-less) hosted store. Recovery namespaces the roll-forward
    /// by it so a crash mid-commit promotes into the right org. Defaulted for
    /// back-compat with journals written before org registries existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) org: Option<String>,
    /// File inside the transaction directory holding the computed
    /// document bytes.
    #[serde(alias = "packument_file")]
    pub(super) document_file: String,
    #[serde(alias = "tarballs")]
    pub(super) blobs: Vec<ManifestBlob>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) revision_refs: Vec<JournaledRevisionRef>,
}

impl ManifestPackage {
    pub(super) fn id(&self) -> PackageId {
        PackageId { ecosystem: self.ecosystem, name: self.name.clone() }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct ManifestBlob {
    /// Canonical on-disk filename (`<basename>-<version>.tgz` for npm).
    pub(super) filename: String,
    /// The staged tmp file holding the verified bytes.
    pub(super) tmp_path: PathBuf,
}
