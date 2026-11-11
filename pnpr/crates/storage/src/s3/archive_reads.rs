use super::S3Store;
use crate::{
    RangedBlob,
    archive_verification::{self, Fingerprint},
    streaming,
};
use axum::body::Body;
use futures_util::StreamExt as _;
use object_store::{GetOptions, GetRange, GetResult, ObjectMeta, ObjectStoreExt as _};
use pnpr_error::Result;
use tokio::{
    fs,
    io::{AsyncReadExt as _, AsyncSeekExt as _, AsyncWriteExt as _},
};

impl S3Store {
    /// Open an archive whose complete bytes have passed integrity verification.
    pub async fn open_archive(&self, key: &str) -> Result<Option<(Body, Option<u64>)>> {
        let result = match self.store.get(&self.archive_key(key)).await {
            Ok(result) => result,
            Err(object_store::Error::NotFound { .. }) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let size = result.meta.size;
        let body = if self.has_verified_archive(&result.meta) {
            Body::from_stream(result.into_stream())
        } else {
            let (file, permit) = self.verify_and_remember(result, key).await?;
            retain_download_permit(streaming::stream_file(file), permit)
        };
        Ok(Some((body, Some(size))))
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
        let result = self.store.get_opts(&location, options).await?;
        let body = if is_verified {
            Body::from_stream(result.into_stream())
        } else {
            self.verified_range_body(result, key, range.clone()).await?
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
        let cache_key = format!("{}:{}", self.verification_id, meta.location);
        let fingerprint = meta.e_tag
            .as_ref()
            .map(|etag| Fingerprint::Remote { size: meta.size, etag: etag.clone() });
        (cache_key, fingerprint)
    }

    async fn verified_range_body(
        &self,
        result: GetResult,
        key: &str,
        range: std::ops::Range<u64>,
    ) -> Result<Body> {
        let (mut file, permit) = self.verify_and_remember(result, key).await?;
        file.seek(std::io::SeekFrom::Start(range.start)).await?;
        Ok(retain_download_permit(
            streaming::stream_file(file.take(range.end - range.start)),
            permit,
        ))
    }

    async fn verify_and_remember(
        &self,
        result: GetResult,
        key: &str,
    ) -> Result<(fs::File, tokio::sync::SemaphorePermit<'static>)> {
        let (cache_key, fingerprint) = self.archive_fingerprint(&result.meta);
        let file = self.verify_archive(result, key).await?;
        if let Some(fingerprint) = fingerprint {
            archive_verification::remember(cache_key, fingerprint);
        }
        Ok(file)
    }

    async fn verify_archive(
        &self,
        result: GetResult,
        key: &str,
    ) -> Result<(fs::File, tokio::sync::SemaphorePermit<'static>)> {
        static COLD_DOWNLOADS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
        let permit = COLD_DOWNLOADS.acquire().await.expect("verification semaphore remains open");
        fs::create_dir_all(&self.staging_dir).await?;
        let directory = self.staging_dir.clone();
        let file = tokio::task::spawn_blocking(move || tempfile::tempfile_in(directory)).await??;
        let mut file = fs::File::from_std(file);
        let mut chunks = result.into_stream();
        while let Some(chunk) = chunks.next().await {
            file.write_all(&chunk?).await?;
        }
        file.flush().await?;
        file.seek(std::io::SeekFrom::Start(0)).await?;
        Ok((archive_verification::verify_file(file, key).await?, permit))
    }
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
