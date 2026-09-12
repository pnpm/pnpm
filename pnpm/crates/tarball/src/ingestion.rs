use crate::{
    ArchiveStoreProjection, FetchedTarball, IgnoreEntryFilter, PrefetchedCasPaths,
    SharedReportedProgressKeys, TarballError, apply_append_manifest, apply_placeholder_manifest,
    download::{download_priority, fetch_and_extract_with_retry, store_index_cache_key},
    emit_progress_found_in_store, load_cached_cas_paths, load_legacy_synthesized_cas_paths,
    local_file_tarball_path,
    zip_archive::fetch_and_extract_zip_with_retry,
};
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pnpm_reporter::Reporter;
use pnpm_store_dir::{
    PackageFilesIndex, SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreDir,
    StoreIndexWriter,
};
use ssri::Integrity;
use std::{collections::HashMap, path::PathBuf, sync::Arc};

#[derive(Clone, Copy)]
pub(crate) enum ArchiveFormat<'a> {
    TarGz { unpacked_size: Option<usize>, file_count: Option<usize>, revision_addressed: bool },
    Zip { integrity: &'a Integrity, prefix: Option<&'a str> },
}

impl ArchiveFormat<'_> {
    fn is_local(self, url: &str) -> bool {
        matches!(self, Self::TarGz { .. }) && local_file_tarball_path(url).is_some()
    }
}

/// Cache reuse and publication are independent of the archive container.
/// A store row is published only after extraction and projection both succeed.
pub(crate) struct ArchiveIngestion<'a> {
    pub(crate) http_client: &'a ThrottledClient,
    pub(crate) store_dir: &'static StoreDir,
    pub(crate) store_index: &'a Option<SharedReadonlyStoreIndex>,
    pub(crate) store_index_writer: &'a Option<Arc<StoreIndexWriter>>,
    pub(crate) verify_store_integrity: bool,
    pub(crate) strict_store_pkg_content_check: bool,
    pub(crate) verified_files_cache: &'a SharedVerifiedFilesCache,
    pub(crate) package_integrity: Option<&'a Integrity>,
    pub(crate) package_url: &'a str,
    pub(crate) package_id: &'a str,
    pub(crate) requester: &'a str,
    pub(crate) prefetched_cas_paths: Option<&'a PrefetchedCasPaths>,
    pub(crate) retry_opts: RetryOpts,
    pub(crate) auth_headers: &'a AuthHeaders,
    pub(crate) ignore_file_pattern: &'a Option<Arc<IgnoreEntryFilter>>,
    pub(crate) offline: bool,
    pub(crate) progress_reported: &'a Option<SharedReportedProgressKeys>,
    pub(crate) store_projection: ArchiveStoreProjection<'a>,
    pub(crate) format: ArchiveFormat<'a>,
}

impl ArchiveIngestion<'_> {
    pub(crate) async fn run<Reporter: self::Reporter>(
        &self,
    ) -> Result<HashMap<String, PathBuf>, TarballError> {
        let cache_key = self.cache_key();
        let progress_key = self.progress_reported.as_ref().zip(cache_key.as_deref());
        if let Some(prefetched) = self.prefetched_cas_paths
            && let Some(cache_key) = cache_key.as_deref()
            && let Some(cas_paths) = prefetched.get(cache_key)
        {
            tracing::info!(
                target: "pacquet::download",
                package_url = ?self.package_url,
                package_id = ?self.package_id,
                "Reusing prefetched CAFS entry — skipping download",
            );
            emit_progress_found_in_store::<Reporter>(self.package_id, self.requester, progress_key);
            return Ok((**cas_paths).clone());
        }
        if let Some(cache_key) = cache_key.clone() {
            let cached = load_cached_cas_paths::<Reporter>(
                self.store_index.clone(),
                self.store_dir,
                cache_key,
                self.verify_store_integrity,
                self.store_projection.package_content_check(self.strict_store_pkg_content_check),
                Arc::clone(self.verified_files_cache),
            )
            .await?;
            if let Some(cas_paths) = cached {
                tracing::info!(target: "pacquet::download", package_url = ?self.package_url, package_id = ?self.package_id, "Reusing cached CAFS entry — skipping download");
                emit_progress_found_in_store::<Reporter>(
                    self.package_id,
                    self.requester,
                    progress_key,
                );
                return Ok(cas_paths);
            }
            if let Some(cas_paths) = self.load_legacy_cache::<Reporter>(progress_key).await? {
                return Ok(cas_paths);
            }
        }
        self.fetch::<Reporter>(false).await.map(|result| result.files_map)
    }

    async fn load_legacy_cache<Reporter: self::Reporter>(
        &self,
        progress_key: Option<(&SharedReportedProgressKeys, &str)>,
    ) -> Result<Option<HashMap<String, PathBuf>>, TarballError> {
        if let (
            Some(package_integrity),
            ArchiveStoreProjection::Package { append_manifest: Some(_) },
        ) = (self.package_integrity, self.store_projection)
        {
            let cached = load_legacy_synthesized_cas_paths::<Reporter>(
                self.store_index.clone(),
                self.store_dir,
                &package_integrity.to_string(),
                self.package_id,
                self.verify_store_integrity,
                Arc::clone(self.verified_files_cache),
                self.store_projection,
            )
            .await?;
            if let Some(cas_paths) = cached {
                tracing::info!(target: "pacquet::download", package_url = ?self.package_url, package_id = ?self.package_id, "Reusing compatible legacy CAFS entry — skipping download");
                emit_progress_found_in_store::<Reporter>(
                    self.package_id,
                    self.requester,
                    progress_key,
                );
                return Ok(Some(cas_paths));
            }
        }
        Ok(None)
    }

    fn cache_key(&self) -> Option<String> {
        store_index_cache_key(self.package_integrity, self.package_id, self.store_projection)
    }

    pub(crate) async fn fetch<Reporter: self::Reporter>(
        &self,
        record_computed_integrity: bool,
    ) -> Result<FetchedTarball, TarballError> {
        let cache_key = self.cache_key();
        if self.offline && !self.format.is_local(self.package_url) {
            tracing::warn!(
                target: "pacquet::download",
                package_url = ?self.package_url,
                package_id = ?self.package_id,
                "offline mode: tarball missing from local store; refusing network fetch",
            );
            return Err(TarballError::NoOfflineTarball {
                package_id: self.package_id.to_string(),
                url: self.package_url.to_string(),
            });
        }

        tracing::info!(target: "pacquet::download", package_url = ?self.package_url, "New cache");

        let (computed_integrity, mut cas_paths, mut pkg_files_idx) = self
            .fetch_archive::<Reporter>(self.progress_reported.as_ref().zip(cache_key.as_deref()))
            .await?;
        self.project_files(&mut cas_paths, &mut pkg_files_idx)?;

        let manifest = pkg_files_idx.manifest.clone();
        let requires_build = match self.format {
            ArchiveFormat::TarGz { .. } => pkg_files_idx
                .requires_build
                .expect("fresh tarball extraction records build requirement"),
            ArchiveFormat::Zip { .. } => pkg_files_idx.requires_build.unwrap_or(false),
        };
        self.queue_index_row(
            cache_key.or_else(|| {
                record_computed_integrity.then(|| {
                    self.store_projection
                        .store_index_key(&computed_integrity.to_string(), self.package_id)
                })
            }),
            pkg_files_idx,
        );

        Ok(FetchedTarball {
            integrity: computed_integrity,
            files_map: cas_paths,
            manifest,
            requires_build,
        })
    }

    async fn fetch_archive<Reporter: self::Reporter>(
        &self,
        progress_key: Option<(&SharedReportedProgressKeys, &str)>,
    ) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
        match self.format {
            ArchiveFormat::TarGz { unpacked_size, file_count, revision_addressed } => {
                fetch_and_extract_with_retry::<Reporter>(
                    self.http_client,
                    self.package_url,
                    self.package_integrity,
                    unpacked_size,
                    download_priority(unpacked_size, file_count),
                    self.package_id,
                    self.requester,
                    self.store_dir,
                    self.retry_opts,
                    self.auth_headers,
                    self.ignore_file_pattern.clone(),
                    progress_key,
                    revision_addressed,
                )
                .await
            }
            ArchiveFormat::Zip { integrity, prefix } => {
                let (paths, index) = fetch_and_extract_zip_with_retry::<Reporter>(
                    self.http_client,
                    self.package_url,
                    integrity,
                    self.package_id,
                    self.requester,
                    self.store_dir,
                    self.retry_opts,
                    self.auth_headers,
                    prefix,
                    self.ignore_file_pattern.clone(),
                )
                .await?;
                Ok((integrity.clone(), paths, index))
            }
        }
    }

    fn project_files(
        &self,
        cas_paths: &mut HashMap<String, PathBuf>,
        pkg_files_idx: &mut PackageFilesIndex,
    ) -> Result<(), TarballError> {
        match self.store_projection {
            ArchiveStoreProjection::Package { append_manifest } => {
                if let Some(manifest_bytes) = append_manifest {
                    apply_append_manifest(
                        self.store_dir,
                        manifest_bytes,
                        cas_paths,
                        pkg_files_idx,
                    )?;
                }
                apply_placeholder_manifest(self.store_dir, cas_paths, pkg_files_idx)
            }
            ArchiveStoreProjection::RawArchive => Ok(()),
        }
    }

    fn queue_index_row(&self, cache_key: Option<String>, pkg_files_idx: PackageFilesIndex) {
        match (cache_key, self.store_index_writer) {
            (Some(index_key), Some(writer)) => writer.queue(index_key, pkg_files_idx),
            (Some(index_key), None) => tracing::warn!(
                target: "pacquet::download",
                ?index_key,
                "no shared store-index writer; skipping index row for this archive",
            ),
            (None, _) => tracing::debug!(
                target: "pacquet::download",
                package_url = ?self.package_url,
                package_id = ?self.package_id,
                "resolution carries no integrity; skipping index row for this archive",
            ),
        }
    }
}
