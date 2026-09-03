//! S3-compatible object-store backend for the **hosted** store.
//!
//! The hosted store is pnpr's source of truth — packages published
//! through its API plus the content served in static mode. When the
//! YAML `s3:` block is present, those authoritative packuments and
//! tarballs live in an object store instead of on local disk, so the
//! durable data can be replicated by the provider and shared by
//! several stateless pnpr replicas.
//!
//! Any S3-compatible endpoint works: AWS S3 (omit `endpoint`),
//! Cloudflare R2 (`region: auto`, the account endpoint), `MinIO`,
//! Backblaze B2, Wasabi, etc. The disposable proxy cache and the
//! resolver `SQLite` stores stay on local disk regardless —
//! only the hosted store is pluggable.

use crate::{
    HOSTED_REVISION_REF_INDEX_FILE, HOSTED_REVISION_REFS_DIR, HostedBackend,
    HostedPackumentForUpdate, HostedPackumentVersion, HostedRevisionRefIndex,
    HostedRevisionRefWrite, PackumentWrite, STAGED_DIR, TarballFinalize, staged_id_of_meta_object,
    wait_after_packument_write_conflict,
};
use async_trait::async_trait;
use axum::body::Body;
use futures_util::StreamExt;
use object_store::{
    ObjectStore, ObjectStoreExt, PutMode, PutOptions, PutPayload, UpdateVersion,
    path::Path as ObjectPath,
};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::PackageName;
use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::fs;

const PACKUMENT_FILE: &str = "package.json";
const REVISION_REF_WRITE_RETRIES: usize = 32;

/// Object-store-backed hosted store. Mirrors the verdaccio-shaped
/// key layout the on-disk [`crate`] uses
/// (`<prefix><pkg>/package.json`, `<prefix><pkg>/<basename>.tgz`) so a
/// bucket and a directory hold the same shape.
#[derive(Debug, Clone)]
pub struct S3Store {
    store: Arc<dyn ObjectStore>,
    /// Normalized prefix: empty or `.../`-terminated.
    prefix: String,
    /// Local directory the publish flow stages decoded tarballs in
    /// before they're uploaded. The decode/verify step writes through
    /// `std::fs` inside `spawn_blocking`, so it needs a real path even
    /// when the final home is a bucket; a subdirectory of the
    /// proxy-cache root doubles as scratch.
    staging_dir: PathBuf,
    /// The proxy-cache root `staging_dir` sits under. The publish journal
    /// lives here, beside the staged tmp files it rolls forward.
    cache_root: PathBuf,
}

#[derive(Debug)]
pub(crate) struct S3PackumentForUpdate {
    pub(crate) bytes: Vec<u8>,
    pub(crate) version: UpdateVersion,
}

/// Subdirectory of the proxy-cache root where hosted tarballs are
/// staged before upload. Its own directory keeps the decode/verify tmp
/// files away from the cache's `<pkg>/` package directories.
const STAGING_SUBDIR: &str = "pnpr-hosted-staging";

impl S3Store {
    pub fn new(store: Arc<dyn ObjectStore>, prefix: String, cache_root: PathBuf) -> Self {
        Self { store, prefix, staging_dir: cache_root.join(STAGING_SUBDIR), cache_root }
    }

    /// A view of this store with `segment` appended to the key prefix, giving a
    /// hosted registry its own object-key namespace under the same bucket.
    /// Staging scratch is shared (its tmp filenames are already unique).
    #[must_use]
    pub fn namespaced(&self, segment: &str) -> S3Store {
        // An empty segment is the flat root: keep the prefix exactly so it
        // addresses the same object keys as the un-namespaced store, rather than
        // gaining a spurious `/` that points at a different key space.
        if segment.is_empty() {
            return Self {
                store: Arc::clone(&self.store),
                prefix: self.prefix.clone(),
                staging_dir: self.staging_dir.clone(),
                cache_root: self.cache_root.clone(),
            };
        }
        Self {
            store: Arc::clone(&self.store),
            prefix: format!("{}{segment}/", self.prefix),
            staging_dir: self.staging_dir.clone(),
            cache_root: self.cache_root.clone(),
        }
    }

    pub async fn read_packument(&self, name: &PackageName) -> Result<Option<Vec<u8>>> {
        match self.store.get(&self.packument_key(name)).await {
            Ok(result) => Ok(Some(result.bytes().await?.to_vec())),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub(crate) async fn read_packument_for_update(
        &self,
        name: &PackageName,
    ) -> Result<Option<S3PackumentForUpdate>> {
        match self.store.get(&self.packument_key(name)).await {
            Ok(result) => {
                let version = UpdateVersion {
                    e_tag: result.meta.e_tag.clone(),
                    version: result.meta.version.clone(),
                };
                Ok(Some(S3PackumentForUpdate { bytes: result.bytes().await?.to_vec(), version }))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub(crate) async fn write_packument_if_current(
        &self,
        name: &PackageName,
        bytes: &[u8],
        version: Option<&UpdateVersion>,
    ) -> Result<bool> {
        let mode = match version {
            Some(version) => PutMode::Update(version.clone()),
            None => PutMode::Create,
        };
        match self
            .store
            .put_opts(
                &self.packument_key(name),
                PutPayload::from(bytes.to_vec()),
                PutOptions { mode, ..PutOptions::default() },
            )
            .await
        {
            Ok(_) => Ok(true),
            Err(
                object_store::Error::AlreadyExists { .. }
                | object_store::Error::NotFound { .. }
                | object_store::Error::Precondition { .. },
            ) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    /// Open a hosted tarball for streaming. `Ok(None)` means the object
    /// doesn't exist so the caller can fall through to the proxy cache
    /// or upstream.
    pub async fn open_tarball(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<Option<(Body, Option<u64>)>> {
        match self.store.get(&self.tarball_key(name, filename)).await {
            Ok(result) => {
                let len = result.meta.size;
                let stream = result
                    .into_stream()
                    .map(|chunk| chunk.map_err(|err| io::Error::other(err.to_string())));
                Ok(Some((Body::from_stream(stream), Some(len))))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// Reserve a local staging path for the publish flow to decode and
    /// verify a tarball into; [`Self::upload_tarball`] promotes it to
    /// the bucket once the verification passes.
    pub async fn staging_tmp_path(&self, _name: &PackageName, filename: &str) -> Result<PathBuf> {
        fs::create_dir_all(&self.staging_dir).await?;
        Ok(crate::unique_tmp_path(&self.staging_dir.join(filename)))
    }

    pub async fn upload_tarball(
        &self,
        tmp_path: &Path,
        name: &PackageName,
        filename: &str,
    ) -> Result<TarballFinalize> {
        let bytes = fs::read(tmp_path).await?;
        let key = self.tarball_key(name, filename);
        // Create-only. A published version's tarball is immutable, so an object
        // already at this key belongs to a concurrent publisher of the same
        // version. Overwriting it would corrupt that artifact against the
        // integrity its packument records, so tolerate only byte-identical
        // content and otherwise report a conflict.
        match self
            .store
            .put_opts(
                &key,
                PutPayload::from(bytes),
                PutOptions { mode: PutMode::Create, ..PutOptions::default() },
            )
            .await
        {
            Ok(_) => Ok(TarballFinalize::Written),
            Err(
                object_store::Error::AlreadyExists { .. }
                | object_store::Error::Precondition { .. },
            ) => {
                let ours = fs::read(tmp_path).await?;
                let existing = self.store.get(&key).await?.bytes().await?;
                if existing.as_ref() == ours.as_slice() {
                    Ok(TarballFinalize::AlreadyIdentical)
                } else {
                    Ok(TarballFinalize::Conflict)
                }
            }
            Err(err) => Err(err.into()),
        }
    }

    pub async fn remove_tarball(&self, name: &PackageName, filename: &str) -> Result<bool> {
        match self.store.delete(&self.tarball_key(name, filename)).await {
            Ok(()) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn remove_package(&self, name: &PackageName) -> Result<bool> {
        let prefix = ObjectPath::from(format!("{}{}/", self.prefix, name.as_str()));
        let mut listing = self.store.list(Some(&prefix));
        let mut removed = false;
        while let Some(meta) = listing.next().await {
            let meta = meta?;
            self.store.delete(&meta.location).await?;
            removed = true;
        }
        Ok(removed)
    }

    /// List the hosted package names (verdaccio-shaped: a name is a
    /// directory holding a `package.json`). Backs the local search
    /// endpoint when the hosted store lives in a bucket.
    pub async fn list_package_names(&self) -> Result<Vec<String>> {
        let scope = (!self.prefix.is_empty())
            .then(|| ObjectPath::from(self.prefix.trim_end_matches('/').to_string()));
        let mut listing = self.store.list(scope.as_ref());
        let mut names = Vec::new();
        while let Some(meta) = listing.next().await {
            let meta = meta?;
            let key = meta.location.as_ref();
            // Skip anything that isn't actually under our prefix rather
            // than falling back to the full key, which would synthesize
            // a wrong name. (Empty prefix strips to the whole key.)
            let Some(rest) = key.strip_prefix(self.prefix.as_str()) else {
                continue;
            };
            if let Some(name) = rest.strip_suffix(&format!("/{PACKUMENT_FILE}")) {
                names.push(name.to_string());
            }
        }
        Ok(names)
    }

    pub async fn read_revision_refs(&self, digest: &str) -> Result<Vec<Vec<u8>>> {
        let Some((index, _)) = self.read_revision_ref_index(digest).await? else {
            return Ok(Vec::new());
        };
        Ok(index.bodies().map(<[u8]>::to_vec).collect())
    }

    pub async fn write_revision_ref(
        &self,
        digest: &str,
        ref_id: &str,
        owner: &str,
        bytes: &[u8],
    ) -> Result<HostedRevisionRefWrite> {
        for attempt in 0..REVISION_REF_WRITE_RETRIES {
            let current = self.read_revision_ref_index(digest).await?;
            let (mut index, version) = match current {
                Some((index, version)) => (index, Some(version)),
                None => (HostedRevisionRefIndex::default(), None),
            };
            let outcome = index.insert(ref_id, owner, bytes)?;
            if outcome != HostedRevisionRefWrite::Claimed {
                return Ok(outcome);
            }
            let mode = match version {
                Some(version) => PutMode::Update(version),
                None => PutMode::Create,
            };
            match self
                .store
                .put_opts(
                    &self.revision_ref_index_key(digest),
                    PutPayload::from(index.to_bytes()),
                    PutOptions { mode, ..PutOptions::default() },
                )
                .await
            {
                Ok(_) => return Ok(HostedRevisionRefWrite::Claimed),
                Err(
                    object_store::Error::AlreadyExists { .. }
                    | object_store::Error::NotFound { .. }
                    | object_store::Error::Precondition { .. },
                ) => {
                    if attempt + 1 < REVISION_REF_WRITE_RETRIES {
                        wait_after_packument_write_conflict(attempt).await;
                    }
                }
                Err(err) => return Err(err.into()),
            }
        }
        let mut index = self
            .read_revision_ref_index(digest)
            .await?
            .map_or_else(HostedRevisionRefIndex::default, |(index, _)| index);
        let outcome = index.insert(ref_id, owner, bytes)?;
        if outcome != HostedRevisionRefWrite::Claimed {
            return Ok(outcome);
        }
        Err(RegistryError::RevisionReferenceWriteConflict { digest: digest.to_string() })
    }

    pub async fn remove_revision_ref(&self, digest: &str, ref_id: &str, owner: &str) -> Result<()> {
        for attempt in 0..REVISION_REF_WRITE_RETRIES {
            let Some((mut index, version)) = self.read_revision_ref_index(digest).await? else {
                return Ok(());
            };
            if !index.remove_if_owned(ref_id, owner) {
                return Ok(());
            }
            match self
                .store
                .put_opts(
                    &self.revision_ref_index_key(digest),
                    PutPayload::from(index.to_bytes()),
                    PutOptions { mode: PutMode::Update(version), ..PutOptions::default() },
                )
                .await
            {
                Ok(_) => return Ok(()),
                Err(
                    object_store::Error::NotFound { .. } | object_store::Error::Precondition { .. },
                ) => {
                    if attempt + 1 < REVISION_REF_WRITE_RETRIES {
                        wait_after_packument_write_conflict(attempt).await;
                    }
                }
                Err(err) => return Err(err.into()),
            }
        }
        if self
            .read_revision_ref_index(digest)
            .await?
            .is_none_or(|(index, _)| !index.is_owned_by(ref_id, owner))
        {
            return Ok(());
        }
        Err(RegistryError::RevisionReferenceWriteConflict { digest: digest.to_string() })
    }

    pub async fn commit_revision_ref(&self, digest: &str, ref_id: &str, owner: &str) -> Result<()> {
        for attempt in 0..REVISION_REF_WRITE_RETRIES {
            let Some((mut index, version)) = self.read_revision_ref_index(digest).await? else {
                return Err(RegistryError::Internal {
                    reason: "hosted revision reference is missing during commit".to_string(),
                });
            };
            if !index.commit_if_owned(ref_id, owner)? {
                return Ok(());
            }
            match self
                .store
                .put_opts(
                    &self.revision_ref_index_key(digest),
                    PutPayload::from(index.to_bytes()),
                    PutOptions { mode: PutMode::Update(version), ..PutOptions::default() },
                )
                .await
            {
                Ok(_) => return Ok(()),
                Err(
                    object_store::Error::NotFound { .. } | object_store::Error::Precondition { .. },
                ) => {
                    if attempt + 1 < REVISION_REF_WRITE_RETRIES {
                        wait_after_packument_write_conflict(attempt).await;
                    }
                }
                Err(err) => return Err(err.into()),
            }
        }
        let Some((mut index, _)) = self.read_revision_ref_index(digest).await? else {
            return Err(RegistryError::Internal {
                reason: "hosted revision reference is missing during commit".to_string(),
            });
        };
        if !index.commit_if_owned(ref_id, owner)? {
            return Ok(());
        }
        Err(RegistryError::RevisionReferenceWriteConflict { digest: digest.to_string() })
    }

    async fn read_revision_ref_index(
        &self,
        digest: &str,
    ) -> Result<Option<(HostedRevisionRefIndex, UpdateVersion)>> {
        match self.store.get(&self.revision_ref_index_key(digest)).await {
            Ok(result) => {
                let version = UpdateVersion {
                    e_tag: result.meta.e_tag.clone(),
                    version: result.meta.version.clone(),
                };
                let index = HostedRevisionRefIndex::from_bytes(&result.bytes().await?)?;
                Ok(Some((index, version)))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn packument_key(&self, name: &PackageName) -> ObjectPath {
        ObjectPath::from(format!("{}{}/{PACKUMENT_FILE}", self.prefix, name.as_str()))
    }

    fn tarball_key(&self, name: &PackageName, filename: &str) -> ObjectPath {
        ObjectPath::from(format!("{}{}/{filename}", self.prefix, name.as_str()))
    }

    fn revision_ref_index_key(&self, digest: &str) -> ObjectPath {
        ObjectPath::from(format!(
            "{}{HOSTED_REVISION_REFS_DIR}/{digest}/{HOSTED_REVISION_REF_INDEX_FILE}",
            self.prefix,
        ))
    }

    // Staged-publish records (see `storage::Storage`'s staged section for
    // the layout contract shared with the fs backend).

    pub async fn read_staged(&self, object: &str) -> Result<Option<Vec<u8>>> {
        match self.store.get(&self.staged_key(object)).await {
            Ok(result) => Ok(Some(result.bytes().await?.to_vec())),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn write_staged(&self, object: &str, bytes: &[u8]) -> Result<()> {
        self.store.put(&self.staged_key(object), PutPayload::from(bytes.to_vec())).await?;
        Ok(())
    }

    pub async fn remove_staged(&self, object: &str) -> Result<bool> {
        match self.store.delete(&self.staged_key(object)).await {
            Ok(()) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn list_staged_ids(&self) -> Result<Vec<String>> {
        let scope = ObjectPath::from(format!("{}{STAGED_DIR}", self.prefix));
        let mut listing = self.store.list(Some(&scope));
        let mut ids = Vec::new();
        while let Some(meta) = listing.next().await {
            let meta = meta?;
            let Some(object) = meta.location.as_ref().rsplit('/').next() else {
                continue;
            };
            if let Some(id) = staged_id_of_meta_object(object) {
                ids.push(id.to_string());
            }
        }
        Ok(ids)
    }

    fn staged_key(&self, object: &str) -> ObjectPath {
        ObjectPath::from(format!("{}{STAGED_DIR}/{object}", self.prefix))
    }
}

#[cfg(test)]
mod tests;

/// The S3-compatible object-store backend. Packument writes are
/// compare-and-set on the object's version, so concurrent publishers on
/// separate nodes cannot lose each other's writes, and a tarball is promoted
/// by upload rather than rename.
#[async_trait]
impl HostedBackend for S3Store {
    async fn read_packument(&self, name: &PackageName) -> Result<Option<Vec<u8>>> {
        S3Store::read_packument(self, name).await
    }

    async fn read_packument_for_update(
        &self,
        name: &PackageName,
    ) -> Result<Option<HostedPackumentForUpdate>> {
        Ok(S3Store::read_packument_for_update(self, name).await?.map(|packument| {
            HostedPackumentForUpdate {
                bytes: packument.bytes,
                version: HostedPackumentVersion::ObjectVersion(packument.version),
            }
        }))
    }

    async fn write_packument_if_current(
        &self,
        name: &PackageName,
        bytes: &[u8],
        version: Option<&HostedPackumentVersion>,
    ) -> Result<PackumentWrite> {
        let version = match version {
            Some(HostedPackumentVersion::ObjectVersion(version)) => Some(version),
            Some(HostedPackumentVersion::Unversioned) | None => None,
        };
        if S3Store::write_packument_if_current(self, name, bytes, version).await? {
            Ok(PackumentWrite::Written)
        } else {
            Ok(PackumentWrite::Conflict)
        }
    }

    async fn open_tarball(
        &self,
        name: &PackageName,
        filename: &str,
    ) -> Result<Option<(Body, Option<u64>)>> {
        S3Store::open_tarball(self, name, filename).await
    }

    async fn reserve_tarball_tmp(&self, name: &PackageName, filename: &str) -> Result<PathBuf> {
        self.staging_tmp_path(name, filename).await
    }

    async fn finalize_tarball(
        &self,
        tmp_path: &Path,
        name: &PackageName,
        filename: &str,
    ) -> Result<TarballFinalize> {
        let outcome = self.upload_tarball(tmp_path, name, filename).await?;
        // Keep the staged tmp on a Conflict so journal roll-forward can
        // re-detect it and exclude the version whose bytes we don't own;
        // once the object is ours there is nothing left to promote.
        if outcome != TarballFinalize::Conflict {
            let _ = fs::remove_file(tmp_path).await;
        }
        Ok(outcome)
    }

    async fn remove_tarball(&self, name: &PackageName, filename: &str) -> Result<bool> {
        S3Store::remove_tarball(self, name, filename).await
    }

    async fn remove_package(&self, name: &PackageName) -> Result<bool> {
        S3Store::remove_package(self, name).await
    }

    async fn list_package_names(&self) -> Result<Vec<String>> {
        S3Store::list_package_names(self).await
    }

    async fn read_revision_refs(&self, digest: &str) -> Result<Vec<Vec<u8>>> {
        S3Store::read_revision_refs(self, digest).await
    }

    async fn write_revision_ref(
        &self,
        digest: &str,
        ref_id: &str,
        owner: &str,
        bytes: &[u8],
    ) -> Result<HostedRevisionRefWrite> {
        S3Store::write_revision_ref(self, digest, ref_id, owner, bytes).await
    }

    async fn remove_revision_ref(&self, digest: &str, ref_id: &str, owner: &str) -> Result<()> {
        S3Store::remove_revision_ref(self, digest, ref_id, owner).await
    }

    async fn commit_revision_ref(&self, digest: &str, ref_id: &str, owner: &str) -> Result<()> {
        S3Store::commit_revision_ref(self, digest, ref_id, owner).await
    }

    fn namespaced(&self, segment: &str) -> Arc<dyn HostedBackend> {
        Arc::new(S3Store::namespaced(self, segment))
    }

    fn local_scratch_root(&self) -> &Path {
        &self.cache_root
    }

    async fn read_staged(&self, object: &str) -> Result<Option<Vec<u8>>> {
        S3Store::read_staged(self, object).await
    }

    async fn write_staged(&self, object: &str, bytes: &[u8]) -> Result<()> {
        S3Store::write_staged(self, object, bytes).await
    }

    async fn remove_staged(&self, object: &str) -> Result<bool> {
        S3Store::remove_staged(self, object).await
    }

    async fn list_staged_ids(&self) -> Result<Vec<String>> {
        S3Store::list_staged_ids(self).await
    }
}
