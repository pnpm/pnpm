use crate::DocumentWrite;
use async_trait::async_trait;
use axum::body::Body;
use object_store::UpdateVersion;
use pnpr_error::Result;
use pnpr_package_name::{CanonicalPackageName, Ecosystem};
use std::{
    fmt::Debug,
    path::{Path, PathBuf},
    sync::Arc,
};

/// The pluggable store behind a hosted registry: a local directory, an
/// S3-compatible bucket, or anything else that can hold documents,
/// blobs, and staged publishes.
///
/// Only the hosted side is pluggable. The disposable proxy cache is always
/// local, because its contents are re-fetchable and its value is being fast.
///
/// Implementations own the differences the rest of the registry should not
/// see: whether writes are compare-and-set, whether a blob is promoted by
/// rename or by upload, and how a namespace maps onto paths or key prefixes.
#[async_trait]
pub(crate) trait HostedBackend: Debug + Send + Sync {
    /// Check that the hosted store is reachable and ready for operations.
    async fn check_readiness(&self) -> Result<()> {
        Ok(())
    }

    fn upload_store(&self) -> Option<crate::upload::RemoteUploadStore> {
        None
    }

    async fn read_document(&self, name: &CanonicalPackageName) -> Result<Option<Vec<u8>>>;

    /// Read a document together with the token [`Self::write_document_if_current`]
    /// needs to detect a concurrent writer.
    async fn read_document_for_update(
        &self,
        name: &CanonicalPackageName,
    ) -> Result<Option<HostedDocumentForUpdate>>;

    /// Write only if the stored document is still at `version`. A backend
    /// without compare-and-set reports [`DocumentWrite::Written`]
    /// unconditionally — it serializes writers by another means.
    async fn write_document_if_current(
        &self,
        name: &CanonicalPackageName,
        bytes: &[u8],
        version: Option<&HostedDocumentVersion>,
    ) -> Result<DocumentWrite>;

    // --- Archives ----------------------------------------------------------
    //
    // A namespace's archives are stored by key (see [`crate::BlobId::key`]),
    // never by a public file name. A key is written only with the bytes its
    // identity names, so a second write of one is a write of the same bytes.

    async fn open_archive(&self, key: &str) -> Result<Option<(Body, Option<u64>)>>;

    async fn open_archive_range(
        &self,
        key: &str,
        range: &object_store::GetRange,
    ) -> Result<Option<crate::RangedBlob>>;

    /// Reserve the local staging path an archive is written to before its
    /// identity is known. Always local: the bytes are verified and hashed on
    /// the way in, before the backend sees them.
    async fn reserve_archive_tmp(&self) -> Result<PathBuf>;

    /// Store the staged file at `tmp_path` under `key`, consuming it
    /// whether or not it is stored.
    async fn put_archive(&self, tmp_path: &Path, key: &str) -> Result<()>;

    /// The path [`Self::reserve_archive_tmp`] names its staging files
    /// after, so files a process left behind when it stopped can be found.
    fn archive_tmp_base(&self) -> PathBuf;

    /// Remove the archive stored under `key`, reporting whether it was there.
    async fn remove_archive(&self, key: &str) -> Result<bool>;

    /// Every name stored where the namespace keeps its archives, keys and
    /// whatever else is there.
    fn list_archives(&self) -> futures_util::stream::BoxStream<'_, Result<crate::StoredArchive>>;

    /// The names of the top-level directories (key prefixes) of this view.
    async fn list_top_level_dirs(&self) -> Result<Vec<String>>;

    /// A view rooted under `segment`, giving a hosted registry its own
    /// namespace so two orgs hosting the same `name@version` never collide.
    fn namespaced(&self, segment: &str) -> Arc<dyn HostedBackend>;

    /// A view that decodes package directories and object keys with
    /// `ecosystem`'s [`crate::layout::package_segments`] rules.
    fn with_ecosystem(&self, ecosystem: Ecosystem) -> Arc<dyn HostedBackend>;

    /// What separates this backend's objects from another organization's.
    ///
    /// Two views with the same namespace address the same objects, and two
    /// with different ones never do. It names an organization where the local
    /// scratch root cannot: on the object-store backend every organization
    /// stages through one directory, so a scratch path alone says nothing
    /// about who owns what is in it.
    fn namespace(&self) -> String;

    /// The local directory this backend stages blobs in. Local even when the
    /// final home is a bucket: the decode/verify step writes through
    /// `std::fs` and needs a real path.
    fn local_scratch_root(&self) -> &Path;

    // --- Records ---------------------------------------------------------
    //
    // The store also holds what the registry itself keeps beside the packages:
    // staged publishes waiting for approval, pipeline run records. Each such
    // kind owns a reserved namespace of the hosted store — a dot-prefixed
    // segment beside `packages/` — and addresses its records by a key within
    // it. `Storage` names the namespaces; a backend only maps a namespace and
    // key onto a path or an object key.

    async fn read_record(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>>;

    /// Write a record where nothing is stored yet, reporting `false` when the
    /// key is taken. Records that must not be rewritten — an approved-once
    /// staged publish, an append-only run record — are created this way, so a
    /// second writer is told rather than overwriting the first.
    async fn create_record(&self, namespace: &str, key: &str, bytes: &[u8]) -> Result<bool>;

    /// Replace a record only while it still holds `expected`.
    ///
    /// This is what makes a staged approval exclusive: the approving replica
    /// rewrites the record it read, and a replica whose copy is no longer what
    /// the store holds — because another approval claimed it, or a rejection
    /// removed it — gets [`DocumentWrite::Conflict`] instead of acting on a
    /// record that has moved on.
    async fn replace_record_if_current(
        &self,
        namespace: &str,
        key: &str,
        expected: &[u8],
        bytes: &[u8],
    ) -> Result<DocumentWrite>;

    async fn remove_record(&self, namespace: &str, key: &str) -> Result<bool>;

    /// Every key stored in `namespace`, in unspecified order, relative to the
    /// namespace and `/`-separated whatever the backend stores them on.
    async fn list_record_keys(&self, namespace: &str) -> Result<Vec<String>>;
}

#[derive(Debug)]
pub struct HostedDocumentForUpdate {
    pub bytes: Vec<u8>,
    pub version: HostedDocumentVersion,
}

/// What a backend needs to recognize the document it handed out, so a
/// read-modify-write can refuse to clobber a concurrent publisher.
#[derive(Debug, Clone)]
pub enum HostedDocumentVersion {
    /// The backend offers no compare-and-set. It is single-writer by
    /// construction, so there is nothing to compare against.
    Unversioned,
    ObjectVersion(UpdateVersion),
    /// The catalog generation that last changed the package, from a
    /// metadata backend that tracks generations.
    Generation(u64),
}
