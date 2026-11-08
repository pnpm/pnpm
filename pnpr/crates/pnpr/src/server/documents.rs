//! The hosted document each surface keeps per package, and the publish that
//! records a blob in one.
//!
//! A hosted registry keeps one document per package — an npm packument, a
//! crate's index entries, a Python project's file list — in its metadata,
//! beside the blobs it serves. Publishing a blob and recording it in the
//! document has to be one operation: a blob no document mentions is
//! invisible, and a document entry with no blob behind it is a broken
//! download. [`store_hosted_artifact`] makes it one, through the same commit
//! the npm publish flow uses.

use super::{
    AppState, hosted_read_namespace,
    publishing::{StagedBase, StagedPublish, Verification, commit_publishes},
};
use pnpr_cargo::{CRATE_DOCUMENT_FORMAT_VERSION, CrateDocument};
use pnpr_error::RegistryError;
use pnpr_oci::{IMAGE_DOCUMENT_FORMAT_VERSION, ImageDocument};
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_pypi::{PROJECT_DOCUMENT_FORMAT_VERSION, ProjectDocument};
use pnpr_registry::Ecosystem;
use pnpr_storage::{
    BlobId,
    metadata::{ArtifactBlob, BlobLookup},
};

/// The per-project document a hosted registry keeps in its document slot: a
/// crate's index entries, a Python project's file list. Read through
/// [`read_hosted_document`] and written through [`store_hosted_artifact`].
pub(super) trait HostedDocument: Sized {
    /// The ecosystem whose surface publishes this document.
    const ECOSYSTEM: Ecosystem;
    /// The maximum format version this server build supports.
    const FORMAT_VERSION: u32;

    fn format_version(&self) -> u32;
    fn empty(name: &str) -> Self;
    fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error>;
    fn to_bytes(&self) -> Vec<u8>;

    /// Take on every entry of `addition` this document does not already have,
    /// and report whether that added anything. Entries already here win:
    /// what was published in the meantime stays, and only what is genuinely
    /// missing is added.
    fn merge(&mut self, addition: Self) -> bool;
}

impl HostedDocument for CrateDocument {
    const ECOSYSTEM: Ecosystem = Ecosystem::Cargo;
    const FORMAT_VERSION: u32 = CRATE_DOCUMENT_FORMAT_VERSION;

    fn format_version(&self) -> u32 {
        self.format_version
    }

    fn empty(name: &str) -> Self {
        CrateDocument::new(name)
    }

    fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        CrateDocument::parse(bytes)
    }

    fn to_bytes(&self) -> Vec<u8> {
        CrateDocument::to_bytes(self)
    }

    fn merge(&mut self, addition: Self) -> bool {
        let before = self.versions.len();
        for entry in addition.versions {
            if self.version(&entry.vers).is_some() {
                continue;
            }
            // The document carries the name as first published, not the
            // lowercase key it is stored under.
            if self.versions.is_empty() {
                self.name.clone_from(&entry.name);
            }
            self.versions.push(entry);
            // The crates API reports one description per crate, so the
            // newest publish that landed owns it.
            self.description.clone_from(&addition.description);
        }
        self.versions.len() != before
    }
}

impl HostedDocument for ProjectDocument {
    const ECOSYSTEM: Ecosystem = Ecosystem::Pypi;
    const FORMAT_VERSION: u32 = PROJECT_DOCUMENT_FORMAT_VERSION;

    fn format_version(&self) -> u32 {
        self.format_version
    }

    fn empty(name: &str) -> Self {
        ProjectDocument::new(name)
    }

    fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        ProjectDocument::parse(bytes)
    }

    fn to_bytes(&self) -> Vec<u8> {
        ProjectDocument::to_bytes(self)
    }

    fn merge(&mut self, addition: Self) -> bool {
        let before = self.files.len();
        for file in addition.files {
            if self.file(&file.filename).is_some() {
                continue;
            }
            self.files.push(file);
        }
        self.files.len() != before
    }
}

impl HostedDocument for ImageDocument {
    const ECOSYSTEM: Ecosystem = Ecosystem::Oci;
    const FORMAT_VERSION: u32 = IMAGE_DOCUMENT_FORMAT_VERSION;

    fn format_version(&self) -> u32 {
        self.format_version
    }

    fn empty(name: &str) -> Self {
        ImageDocument::new(name)
    }

    fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        ImageDocument::parse(bytes)
    }

    fn to_bytes(&self) -> Vec<u8> {
        ImageDocument::to_bytes(self)
    }

    fn merge(&mut self, addition: Self) -> bool {
        ImageDocument::merge(self, addition)
    }
}

/// The hosted document of `key` through `source`'s read gate: `None` when the
/// project is absent (or masked from `identity`).
pub(super) async fn read_hosted_document<Document: HostedDocument>(
    state: &AppState,
    identity: &Identity,
    source: &str,
    key: &CanonicalPackageName,
) -> Result<Option<Document>, RegistryError> {
    let org = hosted_read_namespace(state, identity, source, key.as_str())?;
    state.inner.storage
        .for_hosted(&org, Document::ECOSYSTEM)
        .read_hosted_document(key)
        .await?
        .map(|bytes| Document::parse(&bytes).map_err(RegistryError::Json))
        .transpose()
}

/// Publish one blob: store `bytes` as `filename` under `key` in the hosted
/// namespace `org` and record `addition` — the document holding just that
/// blob's entry — in the project's document, as one commit.
///
/// Writers of the same key on this instance are serialized by the package
/// lock; `refuse` rejects a document the publish must not land on (an entry
/// already present) before anything is written. Across instances the blob's
/// immutable slot and the document's generation decide: a publish that loses
/// either reports the conflict and commits nothing.
pub(super) async fn store_hosted_artifact<Document: HostedDocument + Send>(
    state: &AppState,
    org: &str,
    key: &CanonicalPackageName,
    filename: &str,
    bytes: &[u8],
    refuse: impl Fn(&Document) -> Result<(), RegistryError> + Send + Sync,
    addition: Document,
) -> Result<(), RegistryError> {
    let _guard = state.inner.locks.packages.lock(key.as_str()).await;
    let staged = stage_hosted_artifact(state, org, key, filename, bytes, &refuse, addition).await?;
    commit_publishes(state, vec![staged]).await
}

/// Everything [`store_hosted_artifact`] does up to the commit: verify the blob
/// may be published, stage its bytes, and compute the document that records
/// it. The caller holds the package lock from before this call until after the
/// commit, which is what makes the read-compute-write sequence safe on this
/// instance.
pub(super) async fn stage_hosted_artifact<Document: HostedDocument + Send>(
    state: &AppState,
    org: &str,
    key: &CanonicalPackageName,
    filename: &str,
    bytes: &[u8],
    refuse: &(impl Fn(&Document) -> Result<(), RegistryError> + Send + Sync),
    addition: Document,
) -> Result<StagedPublish, RegistryError> {
    let storage = state.inner.storage.for_hosted(org, Document::ECOSYSTEM);
    let lookup = BlobLookup {
        ids: vec![BlobId::of_bytes(Document::ECOSYSTEM, bytes)],
        artifacts: Vec::new(),
    };
    let base = storage.read_hosted_document_for_publish(key, &lookup).await?;
    let artifact = HostedArtifact { filename, bytes, references: Vec::new() };
    stage_hosted_artifact_on(state, org, key, artifact, base, refuse, addition).await
}

/// The archive a hosted publish stages, and the files of the package already
/// stored that it references.
pub(super) struct HostedArtifact<'bytes> {
    pub(super) filename: &'bytes str,
    pub(super) bytes: &'bytes [u8],
    pub(super) references: Vec<ArtifactBlob>,
}

/// [`stage_hosted_artifact`] on `base`, the document read for the publish
/// before anything it references was checked.
pub(super) async fn stage_hosted_artifact_on<Document: HostedDocument + Send>(
    state: &AppState,
    org: &str,
    key: &CanonicalPackageName,
    artifact: HostedArtifact<'_>,
    base: pnpr_storage::metadata::PublishBase,
    refuse: &(impl Fn(&Document) -> Result<(), RegistryError> + Send + Sync),
    addition: Document,
) -> Result<StagedPublish, RegistryError> {
    let storage = state.inner.storage.for_hosted(org, Document::ECOSYSTEM);
    let stored = base.stored;
    let mut document = match &stored {
        Some(stored) => Document::parse(&stored.bytes).map_err(RegistryError::Json)?,
        None => Document::empty(key.as_str()),
    };
    if document.format_version() > Document::FORMAT_VERSION {
        return Err(RegistryError::DocumentFormatTooNew {
            document: key.as_str().to_string(),
            format_version: document.format_version(),
            supported_version: Document::FORMAT_VERSION,
        });
    }
    refuse(&document)?;
    document.merge(addition);

    let slot = storage.reserve_hosted_blob(key, artifact.filename).await?;
    if let Err(err) = tokio::fs::write(&slot.tmp_path, artifact.bytes).await {
        // Nothing owns this slot yet: not a `StagedPublish` the caller could
        // clean up.
        let _ = tokio::fs::remove_file(&slot.tmp_path).await;
        return Err(err.into());
    }
    Ok(StagedPublish {
        name: key.clone(),
        ecosystem: Document::ECOSYSTEM,
        document: document.to_bytes(),
        base: StagedBase {
            version: stored.map(|stored| stored.version),
            packument: None,
            blobs: base.blobs,
            references: artifact.references,
            ..StagedBase::default()
        },
        slots: vec![slot],
        org: Some(org.to_string()),
        // Only an npm document has dist-tags to record a publisher with.
        publisher: None,
        verification: Verification::default(),
    })
}
