use super::S3Store;
use crate::{
    RangedBlob,
    archive_verification::{self, Fingerprint},
    streaming,
};
use axum::body::Body;
use futures_util::StreamExt as _;
use object_store::{GetOptions, GetRange, GetResult, ObjectMeta, ObjectStoreExt as _};
use pnpr_error::{RegistryError, Result};
use tokio::{
    fs,
    io::{AsyncReadExt as _, AsyncSeekExt as _, AsyncWriteExt as _},
};

impl S3Store {
    /// Open an archive whose complete bytes have passed integrity verification.
    pub async fn open_archive(&self, key: &str) -> Result<Option<(Body, Option<u64>)>> {
        match self.try_cached_archive(key).await? {
            CachedArchive::Read { body, size } => Ok(Some((body, Some(size)))),
            CachedArchive::Missing => Ok(None),
            CachedArchive::Miss => self.read_cold_archive(key).await,
        }
    }

    async fn read_cold_archive(&self, key: &str) -> Result<Option<(Body, Option<u64>)>> {
        let staging = acquire_staging_permit(&COLD_DOWNLOADS).await?;
        let result = match self.store.get(&self.archive_key(key)).await {
            Ok(result) => result,
            Err(object_store::Error::NotFound { .. }) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let size = result.meta.size;
        let (file, permit) = self.verify_and_remember(ColdArchive { result, staging }, key).await?;
        let body = retain_download_permit(streaming::stream_file(file), permit);
        Ok(Some((body, Some(size))))
    }

    async fn try_cached_archive(&self, key: &str) -> Result<CachedArchive> {
        let location = self.archive_key(key);
        let cache_key = self.archive_cache_key(&location);
        let Some(fingerprint @ Fingerprint::Remote { .. }) =
            archive_verification::cached_fingerprint(&cache_key)
        else {
            return Ok(CachedArchive::Miss);
        };
        let Fingerprint::Remote { etag, .. } = &fingerprint else { unreachable!() };
        let options = GetOptions { if_match: Some(etag.clone()), ..Default::default() };
        let result = match self.store.get_opts(&location, options).await {
            Ok(result) => result,
            Err(object_store::Error::Precondition { .. }) => return Ok(CachedArchive::Miss),
            Err(object_store::Error::NotFound { .. }) => return Ok(CachedArchive::Missing),
            Err(error) => return Err(error.into()),
        };
        if self.archive_fingerprint(&result.meta).1.as_ref() != Some(&fingerprint) {
            return Ok(CachedArchive::Miss);
        }
        archive_verification::remember(cache_key, fingerprint);
        let size = result.meta.size;
        Ok(CachedArchive::Read { body: Body::from_stream(result.into_stream()), size })
    }

    pub(super) async fn read_archive_range(
        &self,
        key: &str,
        requested: &GetRange,
    ) -> Result<Option<RangedBlob>> {
        let location = self.archive_key(key);
        let meta = match self.store.head(&location).await {
            Ok(meta) => meta,
            Err(object_store::Error::NotFound { .. }) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let size = meta.size;
        let Ok(range) = requested.as_range(size) else {
            return Ok(Some(RangedBlob::Unsatisfiable { size }));
        };
        if range.is_empty() {
            return Ok(Some(RangedBlob::Unsatisfiable { size }));
        }
        let is_verified = self.has_verified_archive(&meta);
        let options = GetOptions {
            range: is_verified.then(|| GetRange::Bounded(range.clone())),
            if_match: meta.e_tag,
            ..Default::default()
        };
        let staging =
            if is_verified { None } else { Some(acquire_staging_permit(&COLD_DOWNLOADS).await?) };
        let result = self.store.get_opts(&location, options).await?;
        let body = match staging {
            None => Body::from_stream(result.into_stream()),
            Some(staging) => {
                self.verified_range_body(ColdArchive { result, staging }, key, range.clone()).await?
            }
        };
        Ok(Some(RangedBlob::Read { body, range, size }))
    }

    fn has_verified_archive(&self, meta: &ObjectMeta) -> bool {
        let (key, fingerprint) = self.archive_fingerprint(meta);
        fingerprint
            .as_ref()
            .is_some_and(|fingerprint| archive_verification::is_verified(&key, fingerprint))
    }

    fn archive_fingerprint(&self, meta: &ObjectMeta) -> (String, Option<Fingerprint>) {
        let cache_key = self.archive_cache_key(&meta.location);
        let fingerprint = meta.e_tag
            .as_ref()
            .map(|etag| Fingerprint::Remote { size: meta.size, etag: etag.clone() });
        (cache_key, fingerprint)
    }

    fn archive_cache_key(&self, location: &object_store::path::Path) -> String {
        format!("{}:{location}", self.verification_id)
    }

    async fn verified_range_body(
        &self,
        archive: ColdArchive,
        key: &str,
        range: std::ops::Range<u64>,
    ) -> Result<Body> {
        let (mut file, permit) = self.verify_and_remember(archive, key).await?;
        file.seek(std::io::SeekFrom::Start(range.start)).await?;
        Ok(retain_download_permit(
            streaming::stream_file(file.take(range.end - range.start)),
            permit,
        ))
    }

    async fn verify_and_remember(
        &self,
        archive: ColdArchive,
        key: &str,
    ) -> Result<(fs::File, tokio::sync::SemaphorePermit<'static>)> {
        let (cache_key, fingerprint) = self.archive_fingerprint(&archive.result.meta);
        let file = self.verify_archive(archive, key).await?;
        if let Some(fingerprint) = fingerprint {
            archive_verification::remember(cache_key, fingerprint);
        }
        Ok(file)
    }

    async fn verify_archive(
        &self,
        archive: ColdArchive,
        key: &str,
    ) -> Result<(fs::File, tokio::sync::SemaphorePermit<'static>)> {
        let ColdArchive { result, staging } = archive;
        static SCRATCH_SPACE: tokio::sync::Semaphore =
            tokio::sync::Semaphore::const_new(SCRATCH_UNITS as usize);
        let scratch = reserve_scratch_space(&SCRATCH_SPACE, result.meta.size)?;
        fs::create_dir_all(&self.staging_dir).await?;
        let directory = self.staging_dir.clone();
        let file = tokio::task::spawn_blocking(move || tempfile::tempfile_in(directory)).await??;
        let file = spool_archive(result, fs::File::from_std(file), key).await?;
        let (file, (scratch, staging)) =
            archive_verification::verify_file_with_guard(file, key, (scratch, staging)).await?;
        drop(staging);
        Ok((file, scratch))
    }
}

struct ColdArchive {
    result: GetResult,
    staging: tokio::sync::SemaphorePermit<'static>,
}

enum CachedArchive {
    Read { body: Body, size: u64 },
    Missing,
    Miss,
}

static COLD_DOWNLOADS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);

async fn spool_archive(result: GetResult, mut file: fs::File, key: &str) -> Result<fs::File> {
    let corrupt = || RegistryError::CorruptArchive { key: key.to_string() };
    let mut remaining = result.meta.size;
    let mut chunks = result.into_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk?;
        remaining = remaining
            .checked_sub(chunk.len() as u64)
            .ok_or_else(corrupt)?;
        file.write_all(&chunk).await?;
    }
    if remaining != 0 {
        return Err(corrupt());
    }
    file.flush().await?;
    file.seek(std::io::SeekFrom::Start(0)).await?;
    Ok(file)
}

const SCRATCH_UNIT_BYTES: u64 = 1024 * 1024;
const SCRATCH_UNITS: u32 = 1024;
const STAGING_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

fn reserve_scratch_space(
    capacity: &'static tokio::sync::Semaphore,
    size: u64,
) -> Result<tokio::sync::SemaphorePermit<'static>> {
    let units = size
        .div_ceil(SCRATCH_UNIT_BYTES)
        .clamp(1, u64::from(SCRATCH_UNITS));
    let units = u32::try_from(units).expect("scratch units fit in u32");
    capacity
        .try_acquire_many(units)
        .map_err(|_| RegistryError::StorageUnavailable {
            reason: "Archive download scratch space is occupied; try again later".to_string(),
        })
}

async fn acquire_staging_permit(
    capacity: &'static tokio::sync::Semaphore,
) -> Result<tokio::sync::SemaphorePermit<'static>> {
    tokio::time::timeout(STAGING_WAIT, capacity.acquire()).await
        .map_err(|_| RegistryError::StorageUnavailable {
            reason: "Archive verification is busy; try again later".to_string(),
        })?
        .map_err(|_| RegistryError::Internal {
            reason: "archive verification semaphore was closed".to_string(),
        })
}

fn retain_download_permit(body: Body, permit: tokio::sync::SemaphorePermit<'static>) -> Body {
    let stream = futures_util::stream::unfold(
        (body.into_data_stream(), permit),
        |(mut stream, permit)| async move {
            stream
                .next()
                .await
                .map(|chunk| (chunk, (stream, permit)))
        },
    );
    Body::from_stream(stream.fuse())
}

#[cfg(test)]
mod tests;
