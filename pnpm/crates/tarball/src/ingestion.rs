use crate::{
    ArchiveStoreProjection, CachedCasPaths, FetchedTarball, IgnoreEntryFilter,
    SharedReportedProgressKeys, TarballError, apply_append_manifest, apply_placeholder_manifest,
    download::{download_priority, fetch_and_extract_with_retry, store_index_cache_key},
    emit_progress_found_in_store, load_cached_cas_paths, load_legacy_synthesized_cas_paths,
    local_file_tarball_path,
    zip_archive::fetch_and_extract_zip_with_retry,
};
use pnpm_reporter::Reporter;
use pnpm_store_dir::PackageFilesIndex;
use ssri::Integrity;
use std::{collections::HashMap, path::PathBuf, sync::Arc};

#[derive(Clone, Copy)]
pub(crate) enum ArchiveFormat<'a> {
    TarGz { unpacked_size: Option<usize>, file_count: Option<usize>, revision_addressed: bool },
    Zip { integrity: &'a Integrity, prefix: Option<&'a str>, max_bytes: Option<usize> },
}

impl ArchiveFormat<'_> {
    fn is_local(self, url: &str) -> bool {
        matches!(self, Self::TarGz { .. }) && local_file_tarball_path(url).is_some()
    }
}

/// Cache reuse and publication are independent of the archive container.
/// A store row is published only after extraction and projection both succeed.
pub(crate) struct ArchiveIngestion<'a> {
    pub(crate) store: &'a crate::ArchiveStoreContext<'a>,
    pub(crate) fetching: crate::ArchiveFetchOptions<'a>,
    pub(crate) package: crate::TarballPackage<'a>,
    pub(crate) requester: &'a str,
    pub(crate) ignore_file_pattern: &'a Option<Arc<IgnoreEntryFilter>>,
    pub(crate) progress_reported: &'a Option<SharedReportedProgressKeys>,
    pub(crate) store_projection: ArchiveStoreProjection<'a>,
    pub(crate) format: ArchiveFormat<'a>,
}

impl ArchiveIngestion<'_> {
    pub(crate) async fn run<Reporter: self::Reporter>(
        &self,
    ) -> Result<HashMap<String, PathBuf>, TarballError> {
        self.load_or_fetch::<Reporter>().await.map(|cached| cached.files)
    }

    pub(crate) async fn load_or_fetch<Reporter: self::Reporter>(
        &self,
    ) -> Result<CachedCasPaths, TarballError> {
        if let Some(cached) = self.load_cache::<Reporter>().await? {
            return Ok(cached);
        }
        self.fetch::<Reporter>(false).await
            .map(|result| CachedCasPaths { files: result.files_map, manifest: result.manifest })
    }

    pub(crate) async fn load_cache<Reporter: self::Reporter>(
        &self,
    ) -> Result<Option<CachedCasPaths>, TarballError> {
        let cache_key = self.cache_key();
        let progress_key = self.progress_reported.as_ref().zip(cache_key.as_deref());
        if let Some(prefetched) = self.store.prefetched_cas_paths
            && let Some(cache_key) = cache_key.as_deref()
            && let Some(cas_paths) = prefetched.get(cache_key)
        {
            tracing::info!(
                target: "pacquet::download",
                package_url = ?self.package.url,
                package_id = ?self.package.id,
                "Reusing prefetched CAFS entry — skipping download",
            );
            emit_progress_found_in_store::<Reporter>(self.package.id, self.requester, progress_key);
            return Ok(Some(CachedCasPaths { files: (**cas_paths).clone(), manifest: None }));
        }
        if let Some(cache_key) = cache_key.clone() {
            let cached = load_cached_cas_paths::<Reporter>(
                self.store.index.clone(),
                self.store.dir,
                cache_key,
                self.store.verify_integrity,
                self.store_projection.package_content_check(self.store.strict_pkg_content_check),
                Arc::clone(&self.store.verified_files_cache),
            )
            .await?;
            if let Some(cached) = cached {
                tracing::info!(target: "pacquet::download", package_url = ?self.package.url, package_id = ?self.package.id, "Reusing cached CAFS entry — skipping download");
                emit_progress_found_in_store::<Reporter>(
                    self.package.id,
                    self.requester,
                    progress_key,
                );
                return Ok(Some(cached));
            }
            if let Some(cas_paths) = self.load_legacy_cache::<Reporter>(progress_key).await? {
                return Ok(Some(CachedCasPaths { files: cas_paths, manifest: None }));
            }
        }
        Ok(None)
    }

    pub(crate) async fn ingest_zip_buffer(
        &self,
        buffer: Vec<u8>,
    ) -> Result<HashMap<String, PathBuf>, TarballError> {
        let ArchiveFormat::Zip { integrity, prefix, max_bytes } = self.format else {
            unreachable!("ZIP buffers require ZIP ingestion")
        };
        crate::zip_archive::check_zip_size(buffer.len() as u64, max_bytes, self.package.url)?;
        let (mut paths, mut index) = crate::zip_archive::ZipExtraction {
            buffer,
            package_integrity: integrity.clone(),
            package_url: self.package.url.to_string(),
            archive_prefix: prefix.map(str::to_string),
            ignore_file_pattern: self.ignore_file_pattern.clone(),
        }
        .run(self.store.dir)
        .await?;
        self.project_files(&mut paths, &mut index)?;
        self.queue_index_row(self.cache_key(), index);
        Ok(paths)
    }

    async fn load_legacy_cache<Reporter: self::Reporter>(
        &self,
        progress_key: Option<(&SharedReportedProgressKeys, &str)>,
    ) -> Result<Option<HashMap<String, PathBuf>>, TarballError> {
        if let (
            Some(package_integrity),
            ArchiveStoreProjection::Package { append_manifest: Some(_) },
        ) = (self.package.integrity, self.store_projection)
        {
            let cached = load_legacy_synthesized_cas_paths::<Reporter>(
                self.store.index.clone(),
                self.store.dir,
                &package_integrity.to_string(),
                self.package.id,
                self.store.verify_integrity,
                Arc::clone(&self.store.verified_files_cache),
                self.store_projection,
            )
            .await?;
            if let Some(cas_paths) = cached {
                tracing::info!(target: "pacquet::download", package_url = ?self.package.url, package_id = ?self.package.id, "Reusing compatible legacy CAFS entry — skipping download");
                emit_progress_found_in_store::<Reporter>(
                    self.package.id,
                    self.requester,
                    progress_key,
                );
                return Ok(Some(cas_paths));
            }
        }
        Ok(None)
    }

    fn cache_key(&self) -> Option<String> {
        store_index_cache_key(self.package.integrity, self.package.id, self.store_projection)
    }

    pub(crate) async fn fetch<Reporter: self::Reporter>(
        &self,
        record_computed_integrity: bool,
    ) -> Result<FetchedTarball, TarballError> {
        let cache_key = self.cache_key();
        self.check_offline_availability()?;

        tracing::info!(target: "pacquet::download", package_url = ?self.package.url, "New cache");

        let (computed_integrity, mut cas_paths, mut pkg_files_idx) = self
            .fetch_archive::<Reporter>(self.progress_reported.as_ref().zip(cache_key.as_deref()))
            .await?;
        self.project_files(&mut cas_paths, &mut pkg_files_idx)?;

        let manifest = pkg_files_idx.manifest.clone();
        let requires_build = match self.format {
            ArchiveFormat::TarGz { .. } => pkg_files_idx.requires_build.expect(
                "fresh tarball extraction records build requirement",
            ),
            ArchiveFormat::Zip { .. } => pkg_files_idx.requires_build.unwrap_or(false),
        };
        self.queue_index_row(
            cache_key.or_else(|| {
                record_computed_integrity.then(|| {
                    self.store_projection.store_index_key(
                        &computed_integrity.to_string(),
                        self.package.id,
                    )
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

    fn check_offline_availability(&self) -> Result<(), TarballError> {
        if self.fetching.offline && !self.format.is_local(self.package.url) {
            tracing::warn!(
                target: "pacquet::download",
                package_url = ?self.package.url,
                package_id = ?self.package.id,
                "offline mode: tarball missing from local store; refusing network fetch",
            );
            return Err(TarballError::NoOfflineTarball {
                package_id: self.package.id.to_string(),
                url: self.package.url.to_string(),
            });
        }
        Ok(())
    }

    async fn fetch_archive<Reporter: self::Reporter>(
        &self,
        progress_key: Option<(&SharedReportedProgressKeys, &str)>,
    ) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
        match self.format {
            ArchiveFormat::TarGz {
                unpacked_size,
                file_count,
                revision_addressed,
            } => {
                fetch_and_extract_with_retry::<Reporter>(
                    self.fetching.http_client,
                    self.package.url,
                    self.package.integrity,
                    unpacked_size,
                    download_priority(unpacked_size, file_count),
                    self.package.id,
                    self.requester,
                    self.store.dir,
                    self.fetching.retry_opts,
                    self.fetching.auth_headers,
                    self.ignore_file_pattern.clone(),
                    progress_key,
                    revision_addressed,
                )
                .await
            }
            ArchiveFormat::Zip { integrity, prefix, max_bytes } => {
                self.fetch_zip::<Reporter>(integrity, prefix, max_bytes).await
            }
        }
    }

    async fn fetch_zip<Reporter: self::Reporter>(
        &self,
        integrity: &Integrity,
        prefix: Option<&str>,
        max_bytes: Option<usize>,
    ) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
        let (paths, index) = fetch_and_extract_zip_with_retry::<Reporter>(
            self.fetching.http_client,
            self.package.url,
            integrity,
            self.package.id,
            self.requester,
            self.store.dir,
            self.fetching.retry_opts,
            self.fetching.auth_headers,
            prefix,
            self.ignore_file_pattern.clone(),
            max_bytes,
        )
        .await?;
        Ok((integrity.clone(), paths, index))
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
                        self.store.dir,
                        manifest_bytes,
                        cas_paths,
                        pkg_files_idx,
                    )?;
                }
                apply_placeholder_manifest(self.store.dir, cas_paths, pkg_files_idx)
            }
            ArchiveStoreProjection::RawArchive => Ok(()),
        }
    }

    fn queue_index_row(&self, cache_key: Option<String>, pkg_files_idx: PackageFilesIndex) {
        match (cache_key, &self.store.index_writer) {
            (Some(index_key), Some(writer)) => writer.queue(index_key, pkg_files_idx),
            (Some(index_key), None) => tracing::warn!(
                target: "pacquet::download",
                ?index_key,
                "no shared store-index writer; skipping index row for this archive",
            ),
            (None, _) => tracing::debug!(
                target: "pacquet::download",
                package_url = ?self.package.url,
                package_id = ?self.package.id,
                "resolution carries no integrity; skipping index row for this archive",
            ),
        }
    }
}
