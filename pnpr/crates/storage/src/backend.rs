use crate::{HostedRevisionRefWrite, PackumentWrite};
use async_trait::async_trait;
use axum::body::Body;
use object_store::UpdateVersion;
use pnpr_error::Result;
use pnpr_package_name::PackageName;
use std::{
    fmt::Debug,
    path::{Path, PathBuf},
    sync::Arc,
};

/// The pluggable store behind a hosted registry: a local directory, an
/// S3-compatible bucket, or anything else that can hold packuments,
/// tarballs, revision references, and staged publishes.
///
/// Only the hosted side is pluggable. The disposable proxy cache is always
/// local, because its contents are re-fetchable and its value is being fast.
///
/// Implementations own the differences the rest of the registry should not
/// see: whether writes are compare-and-set, whether a tarball is promoted by
/// rename or by upload, and how a namespace maps onto paths or key prefixes.
#[async_trait]
pub(crate) trait HostedBackend: Debug + Send + Sync {
    async fn read_packument(&self, name: &PackageName) -> Result<Option<Vec<u8>>>;

    /// Read a packument together with the token [`Self::write_packument_if_current`]
    /// needs to detect a concurrent writer.
    async fn read_packument_for_update(
        &self,
        name: &PackageName,
    ) -> Result<Option<HostedPackumentForUpdate>>;

    /// Write only if the stored packument is still at `version`. A backend
    /// without compare-and-set reports [`PackumentWrite::Written`]
    /// unconditionally — it serializes writers by another means.
    async fn write_packument_if_current(
        &self,
        name: &PackageName,
        bytes: &[u8],
        version: Option<&HostedPackumentVersion>,
    ) -> Result<PackumentWrite>;

    async fn open_tarball(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<Option<(Body, Option<u64>)>>;

    /// Reserve the local staging path the publish flow decodes into. Always
    /// local: the bytes are verified on the way in, before the backend sees
    /// them.
    async fn reserve_tarball_tmp(&self, name: &PackageName, filename: &str) -> Result<PathBuf>;

    /// Promote a staged tarball to its final home, consuming the tmp file
    /// unless the outcome is [`TarballFinalize::Conflict`] — those bytes stay
    /// put so journal roll-forward can re-detect them.
    async fn finalize_tarball(
        &self,
        tmp_path: &Path,
        name: &PackageName,
        filename: &str,
    ) -> Result<TarballFinalize>;

    async fn remove_tarball(&self, name: &PackageName, filename: &str) -> Result<bool>;

    async fn remove_package(&self, name: &PackageName) -> Result<bool>;

    async fn list_package_names(&self) -> Result<Vec<String>>;

    async fn read_revision_refs(&self, digest: &str) -> Result<Vec<Vec<u8>>>;

    async fn write_revision_ref(
        &self,
        digest: &str,
        ref_id: &str,
        owner: &str,
        bytes: &[u8],
    ) -> Result<HostedRevisionRefWrite>;

    async fn remove_revision_ref(&self, digest: &str, ref_id: &str, owner: &str) -> Result<()>;

    async fn commit_revision_ref(&self, digest: &str, ref_id: &str, owner: &str) -> Result<()>;

    /// A view rooted under `segment`, giving a hosted registry its own
    /// namespace so two orgs hosting the same `name@version` never collide.
    fn namespaced(&self, segment: &str) -> Arc<dyn HostedBackend>;

    /// The local directory this backend stages tarballs in, and with them the
    /// commit journal that rolls a staged publish forward after a crash. Local
    /// even when the final home is a bucket: the decode/verify step writes
    /// through `std::fs` and needs a real path.
    fn local_scratch_root(&self) -> &Path;

    async fn read_staged(&self, object: &str) -> Result<Option<Vec<u8>>>;

    async fn write_staged(&self, object: &str, bytes: &[u8]) -> Result<()>;

    async fn remove_staged(&self, object: &str) -> Result<bool>;

    async fn list_staged_ids(&self) -> Result<Vec<String>>;
}

#[derive(Debug)]
pub struct HostedPackumentForUpdate {
    pub bytes: Vec<u8>,
    pub version: HostedPackumentVersion,
}

/// What a backend needs to recognize the packument it handed out, so a
/// read-modify-write can refuse to clobber a concurrent publisher.
#[derive(Debug)]
pub enum HostedPackumentVersion {
    /// The backend offers no compare-and-set. It is single-writer by
    /// construction, so there is nothing to compare against.
    Unversioned,
    ObjectVersion(UpdateVersion),
}

/// Outcome of promoting a staged tarball into the hosted store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TarballFinalize {
    /// The tarball was promoted: created on S3, or renamed into place on the
    /// single-node FS backend, which owns its store exclusively.
    Written,
    /// An object with byte-identical content already occupied the key, so
    /// promotion was a no-op. Safe — the published artifact is exactly ours.
    AlreadyIdentical,
    /// A *different* object already occupies the key: a concurrent publisher
    /// won this version's tarball. A published version's tarball is immutable,
    /// so the caller must not overwrite it and should surface a write conflict
    /// rather than advertise an integrity that no longer matches the bytes.
    Conflict,
}
