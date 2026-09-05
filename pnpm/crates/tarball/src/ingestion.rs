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
    SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreDir, StoreIndexWriter,
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
        let &ArchiveIngestion {
            store_dir,
            package_integrity,
            package_url,
            package_id,
            requester,
            verify_store_integrity,
            strict_store_pkg_content_check,
            prefetched_cas_paths,
            store_projection,
            ..
        } = self;
        let cache_key = store_index_cache_key(package_integrity, package_id, store_projection);
        let progress_key = self.progress_reported.as_ref().zip(cache_key.as_deref());
        if let Some(prefetched) = prefetched_cas_paths
            && let Some(cache_key) = cache_key.as_deref()
            && let Some(cas_paths) = prefetched.get(cache_key)
        {
            tracing::info!(
                target: "pacquet::download",
                ?package_url,
                ?package_id,
                "Reusing prefetched CAFS entry — skipping download",
            );
            emit_progress_found_in_store::<Reporter>(package_id, requester, progress_key);
            return Ok((**cas_paths).clone());
        }
        if let Some(cache_key) = cache_key.clone() {
            let cached = load_cached_cas_paths::<Reporter>(
                self.store_index.clone(),
                store_dir,
                cache_key,
                verify_store_integrity,
                store_projection.package_content_check(strict_store_pkg_content_check),
                Arc::clone(self.verified_files_cache),
            )
            .await?;
            if let Some(cas_paths) = cached {
                tracing::info!(target: "pacquet::download", ?package_url, ?package_id, "Reusing cached CAFS entry — skipping download");
                emit_progress_found_in_store::<Reporter>(package_id, requester, progress_key);
                return Ok(cas_paths);
            }
            if let (
                Some(package_integrity),
                ArchiveStoreProjection::Package { append_manifest: Some(_) },
            ) = (package_integrity, store_projection)
            {
                let cached = load_legacy_synthesized_cas_paths::<Reporter>(
                    self.store_index.clone(),
                    store_dir,
                    &package_integrity.to_string(),
                    package_id,
                    verify_store_integrity,
                    Arc::clone(self.verified_files_cache),
                    store_projection,
                )
                .await?;
                if let Some(cas_paths) = cached {
                    tracing::info!(target: "pacquet::download", ?package_url, ?package_id, "Reusing compatible legacy CAFS entry — skipping download");
                    emit_progress_found_in_store::<Reporter>(package_id, requester, progress_key);
                    return Ok(cas_paths);
                }
            }
        }
        self.fetch::<Reporter>(false).await.map(|result| result.files_map)
    }

    pub(crate) async fn fetch<Reporter: self::Reporter>(
        &self,
        record_computed_integrity: bool,
    ) -> Result<FetchedTarball, TarballError> {
        let &ArchiveIngestion {
            http_client,
            store_dir,
            package_integrity,
            format,
            package_url,
            package_id,
            requester,
            retry_opts,
            auth_headers,
            store_projection,
            ..
        } = self;
        let cache_key = store_index_cache_key(package_integrity, package_id, store_projection);
        let progress_key = self.progress_reported.as_ref().zip(cache_key.as_deref());
        let store_index_writer = self.store_index_writer.clone();
        let ignore_file_pattern = self.ignore_file_pattern.clone();
        if self.offline && !format.is_local(package_url) {
            tracing::warn!(
                target: "pacquet::download",
                ?package_url,
                ?package_id,
                "offline mode: tarball missing from local store; refusing network fetch",
            );
            return Err(TarballError::NoOfflineTarball {
                package_id: package_id.to_string(),
                url: package_url.to_string(),
            });
        }

        tracing::info!(target: "pacquet::download", ?package_url, "New cache");

        let (computed_integrity, mut cas_paths, mut pkg_files_idx) = match format {
            ArchiveFormat::TarGz { unpacked_size, file_count, revision_addressed } => {
                fetch_and_extract_with_retry::<Reporter>(
                    http_client,
                    package_url,
                    package_integrity,
                    unpacked_size,
                    download_priority(unpacked_size, file_count),
                    package_id,
                    requester,
                    store_dir,
                    retry_opts,
                    auth_headers,
                    ignore_file_pattern,
                    progress_key,
                    revision_addressed,
                )
                .await?
            }
            ArchiveFormat::Zip { integrity, prefix } => {
                let (paths, index) = fetch_and_extract_zip_with_retry::<Reporter>(
                    http_client,
                    package_url,
                    integrity,
                    package_id,
                    requester,
                    store_dir,
                    retry_opts,
                    auth_headers,
                    prefix,
                    ignore_file_pattern,
                )
                .await?;
                (integrity.clone(), paths, index)
            }
        };

        match store_projection {
            ArchiveStoreProjection::Package { append_manifest } => {
                if let Some(manifest_bytes) = append_manifest {
                    apply_append_manifest(
                        store_dir,
                        manifest_bytes,
                        &mut cas_paths,
                        &mut pkg_files_idx,
                    )?;
                }
                apply_placeholder_manifest(store_dir, &mut cas_paths, &mut pkg_files_idx)?;
            }
            ArchiveStoreProjection::RawArchive => {}
        }

        let manifest = pkg_files_idx.manifest.clone();
        let requires_build = match format {
            ArchiveFormat::TarGz { .. } => pkg_files_idx
                .requires_build
                .expect("fresh tarball extraction records build requirement"),
            ArchiveFormat::Zip { .. } => pkg_files_idx.requires_build.unwrap_or(false),
        };
        let cache_key = cache_key.or_else(|| {
            record_computed_integrity.then(|| {
                store_projection.store_index_key(&computed_integrity.to_string(), package_id)
            })
        });
        match (cache_key, store_index_writer) {
            (Some(index_key), Some(writer)) => writer.queue(index_key, pkg_files_idx),
            (Some(index_key), None) => tracing::warn!(
                target: "pacquet::download",
                ?index_key,
                "no shared store-index writer; skipping index row for this archive",
            ),
            (None, _) => tracing::debug!(
                target: "pacquet::download",
                ?package_url,
                ?package_id,
                "resolution carries no integrity; skipping index row for this archive",
            ),
        }

        Ok(FetchedTarball {
            integrity: computed_integrity,
            files_map: cas_paths,
            manifest,
            requires_build,
        })
    }
}
