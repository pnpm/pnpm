use super::{
    Arc, AuthHeaders, BodyProgress, BodyVerification, BufferBody, Buffered, BufferedArchive,
    FetchingProgressLog, FetchingProgressMessage, HashMap, IgnoreEntryFilter, LogEvent, LogLevel,
    NetworkError, PackageFilesIndex, Path, PathBuf, Reporter, SemaphorePermit, StoreDir, Stream,
    TarballError, ThrottledClient, buffer_body, extract_body_while_downloading,
    extract_tarball_buffer, local_file_tarball_path, open_local_tarball, read_local_tarball_buffer,
    streaming_extract_semaphore, streams_while_downloading,
};
use crate::{
    TarLimits, archive_integrity::ArchiveVerification, content_coding::ContentCoding,
    download::buffered::ExtractedFiles,
};
use pnpm_package_manifest::PackageBin;
use std::collections::BTreeMap;

/// Run one full tarball-fetch attempt: network, body, integrity hash,
/// decompress, extract into the CAFS. Returns the cas-paths map and the
/// [`PackageFilesIndex`] row the caller queues once the retry loop
/// succeeds.
///
/// One attempt spans the whole pipeline so that a post-download failure
/// — integrity mismatch, gzip decode, malformed tar — reaches the retry
/// boundary, where a re-fetch can recover from a transfer that happened
/// to checksum or decode wrong.
///
/// Permits are acquired inside the attempt so a backoff sleep never
/// parks one. The network permit spans connect through body streaming
/// (pnpm's pQueue, and [#281]'s EMFILE fix) and is dropped before
/// [`post_download_semaphore`](crate::post_download_semaphore) gates the CPU-bound tail.
///
/// [#281]: https://github.com/pnpm/pacquet/pull/281
#[expect(clippy::too_many_arguments, reason = "arg count is fixed by the fetcher signature")]
pub(crate) async fn fetch_and_extract_once<Reporter: self::Reporter>(
    http_client: &ThrottledClient,
    package_url: &str,
    verification: &ArchiveVerification,
    package_unpacked_size: Option<usize>,
    tar_limits: TarLimits,
    download_priority: u64,
    package_id: &str,
    attempt: u32,
    store_dir: &'static StoreDir,
    auth_headers: &AuthHeaders,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
    revision_addressed: bool,
    if_none_match: Option<&str>,
) -> Result<AttemptedFetch, TarballError> {
    let download = TarballDownload {
        http_client,
        package_url,
        verification,
        package_unpacked_size,
        tar_limits,
        package_id,
        store_dir,
        ignore_file_pattern,
    };
    if let Some(path) = local_file_tarball_path(package_url) {
        return fetch_local_archive::<Reporter>(download, &path, attempt).await;
    }
    fetch_remote::<Reporter>(
        download,
        auth_headers,
        download_priority,
        attempt,
        revision_addressed,
        if_none_match,
    )
    .await
}

async fn fetch_local_archive<Reporter: self::Reporter>(
    download: TarballDownload<'_>,
    path: &Path,
    attempt: u32,
) -> Result<AttemptedFetch, TarballError> {
    let package_url = download.package_url;
    let (integrity, files, index, bins) = download.fetch_local::<Reporter>(path, attempt).await?;
    Ok(AttemptedFetch::Extracted(Box::new(ExtractedArchive {
        integrity,
        files,
        index,
        bins,
        meta: empty_meta(package_url),
    })))
}

async fn fetch_remote<Reporter: self::Reporter>(
    download: TarballDownload<'_>,
    auth_headers: &AuthHeaders,
    download_priority: u64,
    attempt: u32,
    revision_addressed: bool,
    if_none_match: Option<&str>,
) -> Result<AttemptedFetch, TarballError> {
    let (client, response_head, meta) = crate::archive_request::request_archive::<Reporter>(
        download.http_client,
        download.package_url,
        download.package_id,
        auth_headers,
        download_priority,
        attempt,
        revision_addressed,
        if_none_match,
        crate::archive_request::ArchiveContentCoding::Negotiated,
    )
    .await?;
    if meta.not_modified {
        drop(response_head);
        return Ok(AttemptedFetch::NotModified(meta));
    }
    let (integrity, files, index, bins) =
        download.extract_response::<Reporter, _>(client, response_head, attempt).await?;
    Ok(AttemptedFetch::Extracted(Box::new(ExtractedArchive {
        integrity,
        files,
        index,
        bins,
        meta,
    })))
}

pub(crate) struct ExtractedArchive {
    pub integrity: crate::ArchiveIntegrity,
    pub files: HashMap<String, PathBuf>,
    pub index: PackageFilesIndex,
    pub bins: Option<BTreeMap<String, PackageBin>>,
    pub meta: crate::archive_request::ArchiveResponseMeta,
}

pub(crate) enum AttemptedFetch {
    Extracted(Box<ExtractedArchive>),
    NotModified(crate::archive_request::ArchiveResponseMeta),
}

fn empty_meta(package_url: &str) -> crate::archive_request::ArchiveResponseMeta {
    crate::archive_request::ArchiveResponseMeta {
        not_modified: false,
        cache_headers: crate::CacheHeaders::default(),
        final_url: package_url.to_owned(),
    }
}

pub(super) struct TarballDownload<'a> {
    pub(super) http_client: &'a ThrottledClient,
    pub(super) package_url: &'a str,
    pub(super) verification: &'a ArchiveVerification,
    pub(super) package_unpacked_size: Option<usize>,
    pub(super) tar_limits: TarLimits,
    pub(super) package_id: &'a str,
    pub(super) store_dir: &'static StoreDir,
    pub(super) ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
}

/// A `file:` tarball is read straight off disk: no request, no streaming
/// extractor, and progress is reported once at its known size.
#[expect(
    clippy::too_many_arguments,
    reason = "the parameters are the independent pieces of one fetch; a struct would only move the same fields into a wrapper"
)]
pub(super) async fn fetch_local_tarball<Reporter: self::Reporter>(
    path: &Path,
    verification: &ArchiveVerification,
    package_unpacked_size: Option<usize>,
    tar_limits: TarLimits,
    package_url: &str,
    package_id: &str,
    attempt: u32,
    store_dir: &'static StoreDir,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
) -> Result<ExtractedFiles, TarballError> {
    let (file, size) = open_local_tarball(path).await?;
    Reporter::emit(&LogEvent::FetchingProgress(FetchingProgressLog {
        level: LogLevel::Debug,
        message: FetchingProgressMessage::Started {
            attempt: attempt + 1,
            package_id: package_id.to_owned(),
            size: Some(size),
        },
    }));
    let buffer = read_local_tarball_buffer(file, path, package_url, size).await?;
    extract_tarball_buffer(
        BufferedArchive {
            buffer,
            content_coding: ContentCoding::Identity,
            verification: verification.clone(),
            unpacked_size: package_unpacked_size,
            tar_limits,
        },
        package_url,
        store_dir,
        ignore_file_pattern,
    )
    .await
}

pub(super) fn fetch_error(package_url: &str, error: reqwest::Error) -> TarballError {
    TarballError::FetchTarball(NetworkError::new(package_url, error))
}

impl TarballDownload<'_> {
    pub(super) async fn fetch_local<Reporter: self::Reporter>(
        self,
        path: &Path,
        attempt: u32,
    ) -> Result<ExtractedFiles, TarballError> {
        fetch_local_tarball::<Reporter>(
            path,
            self.verification,
            self.package_unpacked_size,
            self.tar_limits,
            self.package_url,
            self.package_id,
            attempt,
            self.store_dir,
            self.ignore_file_pattern,
        )
        .await
    }

    pub(super) async fn extract_response<Reporter: self::Reporter, Guard>(
        self,
        client: Guard,
        response: reqwest::Response,
        attempt: u32,
    ) -> Result<ExtractedFiles, TarballError> {
        let content_coding = ContentCoding::of_response(&response, self.package_url)?;
        let expected_size = response.content_length();
        let mut stream = response.bytes_stream();
        let mut progress = BodyProgress::new(expected_size, self.package_id);
        // Retries remain buffered to preserve whole-archive decode diagnostics.
        if attempt == 0
            && streams_while_downloading(expected_size, self.package_unpacked_size)
            && let Ok(permit) = streaming_extract_semaphore().try_acquire()
        {
            return self.stream_body::<Reporter, _, _>(
                content_coding,
                Vec::new(),
                stream,
                progress,
                client,
                permit,
            )
            .await;
        }
        let buffered = buffer_body::<Reporter, _>(BufferBody {
            stream: &mut stream,
            progress: &mut progress,
            expected_size,
            package_url: self.package_url,
            http_client: self.http_client,
        })
        .await
        .inspect_err(|error| {
            if error.is_fetch_timeout() {
                self.http_client.downscale_while_peers_active(self.package_url);
            }
        })?;
        self.finish_body::<Reporter, _, _>(content_coding, buffered, stream, progress, client).await
    }

    pub(super) async fn finish_body<Reporter, Body, Guard>(
        self,
        content_coding: ContentCoding,
        buffered: Buffered,
        stream: Body,
        progress: BodyProgress<'_>,
        client: Guard,
    ) -> Result<ExtractedFiles, TarballError>
    where
        Reporter: self::Reporter,
        Body: Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
    {
        let buffer = match buffered {
            Buffered::Complete(buffer) => buffer,
            Buffered::Overflowed(buffer) => {
                return self.stream_overflowed::<Reporter, _, _>(
                    content_coding,
                    buffer,
                    stream,
                    progress,
                    client,
                )
                .await;
            }
        };
        drop(stream);
        // Release the network slot before the CPU-bound extraction gate.
        // Gating buffering with that smaller semaphore would serialize downloads.
        drop(client);
        extract_tarball_buffer(
            BufferedArchive {
                buffer,
                content_coding,
                verification: self.verification.clone(),
                unpacked_size: self.package_unpacked_size,
                tar_limits: self.tar_limits,
            },
            self.package_url,
            self.store_dir,
            self.ignore_file_pattern,
        )
        .await
    }

    async fn stream_overflowed<Reporter, Body, Guard>(
        self,
        content_coding: ContentCoding,
        buffer: Vec<u8>,
        stream: Body,
        progress: BodyProgress<'_>,
        client: Guard,
    ) -> Result<ExtractedFiles, TarballError>
    where
        Reporter: self::Reporter,
        Body: Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
    {
        let permit = streaming_extract_semaphore()
            .acquire()
            .await
            .expect("streaming-extract semaphore shouldn't be closed this soon");
        self.stream_body::<Reporter, _, _>(
            content_coding,
            vec![bytes::Bytes::from(buffer)],
            stream,
            progress,
            client,
            permit,
        )
        .await
    }

    pub(super) async fn stream_body<Reporter, Body, Guard>(
        self,
        content_coding: ContentCoding,
        prefix: Vec<bytes::Bytes>,
        stream: Body,
        mut progress: BodyProgress<'_>,
        client: Guard,
        permit: SemaphorePermit<'static>,
    ) -> Result<ExtractedFiles, TarballError>
    where
        Reporter: self::Reporter,
        Body: Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
    {
        extract_body_while_downloading::<Reporter, _, _>(
            prefix,
            stream,
            BodyVerification {
                content_coding,
                verification: self.verification.clone(),
                limits: self.tar_limits.with_unpacked_size(self.package_unpacked_size),
                store_dir: self.store_dir,
                ignore_file_pattern: self.ignore_file_pattern,
            },
            &mut progress,
            client,
            permit,
            self.http_client,
            self.package_url,
        )
        .await
    }
}
