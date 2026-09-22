//! Download a remote tarball during resolution and share the extraction
//! through the in-memory cache.

use crate::{
    CacheValue,
    CachedTarball,
    MemCache,
    RetryOpts,
    TarballError,
    TarballPackage,
    apply_placeholder_manifest,
    claim_cache_entry,
    download::fetch_and_extract_with_retry,
    package_mem_cache_key,
    publish_cache_failure,
    publish_cached_tarball,
    read_cas_package_json,
    read_subdir_manifest,
    wait_for_cached_tarball,
};
use pnpm_network::{
    AuthHeaders,
    ThrottledClient,
    UNPRIORITIZED,
};
use pnpm_reporter::Reporter;
use pnpm_store_dir::{
    PackageFilesIndex,
    StoreDir,
    StoreIndexWriter,
    store_index_key,
};
use ssri::Integrity;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::{
    Notify,
    RwLock,
};

/// Outcome of [`FetchTarballForResolution::run`]: the sha512 integrity
/// computed from the downloaded tarball and the bundled manifest read
/// from its `package.json`. The extracted CAFS paths are not returned —
/// they are stashed in the shared [`MemCache`] under the archive's
/// computed integrity so the install pass, which records that same hash
/// in the lockfile, reuses them without re-downloading.
#[derive(Debug)]
pub struct ResolvedTarball {
    pub integrity: Integrity,
    pub manifest: Option<serde_json::Value>,
}

/// Download a remote tarball during *resolution*, settle its sha512
/// integrity (computed from the bytes, or checked against the one the
/// caller pins), extract it to the store, and read its bundled manifest.
///
/// Remote (non-registry) https-tarball direct dependencies carry no
/// name/version/integrity at resolve time — those live in the tarball's
/// `package.json`, learned only after the fetch. pacquet builds the
/// lockfile before the install pass, so the `TarballResolver` must
/// fetch here to fill `manifest` + `integrity` into its
/// `ResolveResult`. Passing a `mem_cache` warms it under the hash this
/// fetch settles, so the install pass's
/// [`crate::IngestTarballToStore::run_with_mem_cache`] reuses the
/// extraction without a second download. A pinned read whose archive is
/// already being downloaded parks on that slot and takes the bundled
/// manifest from it.
pub struct FetchTarballForResolution<'a> {
    pub http_client: &'a ThrottledClient,
    pub store_dir: &'static StoreDir,
    pub store_index_writer: Option<Arc<StoreIndexWriter>>,
    /// The archive to read and what is already known about it.
    ///
    /// `id` is used for scoped auth lookup and for the store-index row
    /// this fetch writes; it must be the `pkg_id` the install pass
    /// derives from the lockfile entry — the bare URL for a remote
    /// tarball — or the two passes file the same content under two rows.
    ///
    /// `integrity` is `None` when this fetch is what discovers the hash,
    /// and `Some` when the resolution already pins one and the fetch
    /// only reads the bundled manifest. Leaving a pinned hash out would
    /// let a tampered archive drive the dependency walk.
    pub package: TarballPackage<'a>,
    pub auth_headers: &'a AuthHeaders,
    pub retry_opts: RetryOpts,
    /// Directory *within* the archive holding the package, for a
    /// git-hosted dep that points at one directory of a repo
    /// (`#path:/packages/foo`). The archive spans the whole repo, so
    /// the root `package.json` describes the repo, not the package —
    /// read the manifest from here instead. `None` reads the root.
    ///
    /// Matches the resolution's `path` field verbatim, leading slash
    /// and all.
    ///
    /// Setting this suppresses the store-index row: the extracted
    /// index describes the archive, not the named subpackage, so
    /// there is no row to write that the key would honestly describe.
    pub manifest_subdir: Option<&'a str>,
    /// Whether the resolution pins a registry revision, whose protocol
    /// allows exactly one GET and rejects redirects. This read is that
    /// GET, so it publishes under the revision-addressed cache identity
    /// the install pass looks the archive up by, and the install spends
    /// no second one.
    pub revision_addressed: bool,
}

struct ExtractedTarball {
    integrity: Integrity,
    files: Arc<HashMap<String, PathBuf>>,
    /// Manifest the caller asked for: the archive root, or a subdirectory.
    manifest: Option<serde_json::Value>,
    /// Archive-root bundled subset. The mem-cache slot is keyed by the
    /// archive, so waiters always see this rather than a subdirectory.
    root_manifest: Option<serde_json::Value>,
}

impl ExtractedTarball {
    fn into_resolved(self) -> ResolvedTarball {
        ResolvedTarball { integrity: self.integrity, manifest: self.manifest }
    }
}

impl FetchTarballForResolution<'_> {
    pub async fn run<Reporter: self::Reporter>(
        self,
        mem_cache: Option<&MemCache>,
    ) -> Result<ResolvedTarball, TarballError> {
        match (mem_cache, self.package.integrity) {
            (Some(mem_cache), Some(_)) => self.run_pinned::<Reporter>(mem_cache).await,
            (mem_cache, _) => self.fetch_and_publish::<Reporter>(mem_cache).await,
        }
    }

    /// A pinned read names the archive before the fetch, so it claims
    /// the same slot the install-pass ingest uses. Winning the claim
    /// fetches; losing it parks on the in-flight extraction and reads
    /// the bundled manifest from the settled slot.
    async fn run_pinned<Reporter: self::Reporter>(
        self,
        mem_cache: &MemCache,
    ) -> Result<ResolvedTarball, TarballError> {
        let mem_cache_key = package_mem_cache_key(
            self.package.url,
            self.package.integrity,
            self.revision_addressed,
        );
        let (cache_lock, owner_notify) = claim_cache_entry(mem_cache, mem_cache_key.clone());
        match owner_notify {
            None => self.take_cached(&cache_lock).await,
            Some(notify) => {
                self.fetch_as_owner::<Reporter>(mem_cache, mem_cache_key, cache_lock, notify).await
            }
        }
    }

    async fn take_cached(
        &self,
        cache_lock: &RwLock<CacheValue>,
    ) -> Result<ResolvedTarball, TarballError> {
        let cached = wait_for_cached_tarball(cache_lock, self.package.url).await?;
        let integrity =
            self.package.integrity.cloned().expect("a pinned read claims a hashed cache identity");
        let manifest = self.manifest_from_cached(&cached).await?;
        Ok(ResolvedTarball { integrity, manifest })
    }

    async fn manifest_from_cached(
        &self,
        cached: &CachedTarball,
    ) -> Result<Option<serde_json::Value>, TarballError> {
        match (&self.manifest_subdir, cached.manifest.as_ref()) {
            (Some(subdir), _) => read_subdir_manifest(&cached.files, subdir).await,
            (None, Some(manifest)) => Ok(Some(manifest.clone())),
            (None, None) => read_cas_package_json(&cached.files, "package.json").await,
        }
    }

    async fn fetch_as_owner<Reporter: self::Reporter>(
        self,
        mem_cache: &MemCache,
        mem_cache_key: String,
        cache_lock: Arc<RwLock<CacheValue>>,
        notify: Arc<Notify>,
    ) -> Result<ResolvedTarball, TarballError> {
        match self.fetch_extracted::<Reporter>().await {
            Ok(extracted) => {
                publish_cached_tarball(
                    &cache_lock,
                    &notify,
                    CachedTarball {
                        files: Arc::clone(&extracted.files),
                        manifest: extracted.root_manifest.clone(),
                    },
                )
                .await;
                Ok(extracted.into_resolved())
            }
            Err(err) => {
                publish_cache_failure(
                    mem_cache,
                    &mem_cache_key,
                    &cache_lock,
                    &notify,
                    self.revision_addressed,
                )
                .await;
                Err(err)
            }
        }
    }

    async fn fetch_and_publish<Reporter: self::Reporter>(
        self,
        mem_cache: Option<&MemCache>,
    ) -> Result<ResolvedTarball, TarballError> {
        let extracted = self.fetch_extracted::<Reporter>().await?;
        if let Some(mem_cache) = mem_cache {
            insert_available_if_vacant(
                mem_cache,
                package_mem_cache_key(
                    self.package.url,
                    Some(&extracted.integrity),
                    self.revision_addressed,
                ),
                CachedTarball {
                    files: Arc::clone(&extracted.files),
                    manifest: extracted.root_manifest.clone(),
                },
            );
        }
        Ok(extracted.into_resolved())
    }

    /// Resolve-time tarball fetches compute integrity from bytes and
    /// gate the dependency walk, so they use the same priority class as
    /// packument requests instead of queuing behind sized downloads.
    async fn fetch_extracted<Reporter: self::Reporter>(
        &self,
    ) -> Result<ExtractedTarball, TarballError> {
        let (integrity, mut cas_paths, mut pkg_files_idx) =
            fetch_and_extract_with_retry::<Reporter>(
                self.http_client,
                self.package.url,
                self.package.integrity,
                self.package.unpacked_size,
                UNPRIORITIZED,
                self.package.id,
                self.package.url,
                self.store_dir,
                self.retry_opts,
                self.auth_headers,
                None,
                None,
                self.revision_addressed,
            )
            .await?;
        apply_placeholder_manifest(self.store_dir, &mut cas_paths, &mut pkg_files_idx)?;
        let root_manifest = pkg_files_idx.manifest.clone();
        let manifest = match self.manifest_subdir {
            Some(subdir) => read_subdir_manifest(&cas_paths, subdir).await?,
            None => root_manifest.clone(),
        };
        self.record_store_index_row(&integrity, pkg_files_idx);
        Ok(ExtractedTarball { integrity, files: Arc::new(cas_paths), manifest, root_manifest })
    }

    /// File this extraction under the caller's `package_id` — the same
    /// `pkg_id` the install pass derives from the lockfile entry. Deriving
    /// a `name@version` from the bundled manifest instead would file a
    /// remote tarball under a key nothing ever reads, leaving the install
    /// pass to write a second row for the same content.
    ///
    /// A subdirectory package gets no row. Its key would name the
    /// subpackage while `pkg_files_idx` describes the whole archive
    /// — the repo's manifest and every repo file — and a row whose
    /// key and payload disagree is worse than none: consumers that
    /// trust `PackageFilesIndex.manifest` / `files` to match the key
    /// (bin linking, file materialization) would read the repo.
    /// Nothing needs this row. A git-hosted archive — the only shape
    /// carrying a subdirectory — is addressed by
    /// `git_hosted_store_index_key` once the install pass has run
    /// `prepare` over it, and both the graph prefetch and the
    /// warm-store reuse map skip git-hosted entries.
    fn record_store_index_row(&self, integrity: &Integrity, pkg_files_idx: PackageFilesIndex) {
        if self.manifest_subdir.is_some() {
            return;
        }
        let index_key = store_index_key(&integrity.to_string(), self.package.id);
        if let Some(writer) = &self.store_index_writer {
            writer.queue(index_key, pkg_files_idx);
        } else {
            tracing::warn!(
                target: "pacquet::download",
                ?index_key,
                "no shared store-index writer; skipping index row for this resolve-time tarball",
            );
        }
    }
}

fn insert_available_if_vacant(mem_cache: &MemCache, key: String, cached: CachedTarball) {
    if let dashmap::mapref::entry::Entry::Vacant(entry) = mem_cache.entry(key) {
        entry.insert(Arc::new(RwLock::new(CacheValue::Available(Arc::new(cached)))));
    }
}
