use super::{
    Arc, AuthHeaders, BodyHasher, BodyProgress, BufferBody, Buffered, FetchingProgressLog,
    FetchingProgressMessage, GZIP_MAGIC, HashMap, IgnoreEntryFilter, Integrity, LogEvent, LogLevel,
    NetworkError, PackageFilesIndex, Path, PathBuf, Reporter, SemaphorePermit, StoreDir, Stream,
    StreamExt, TarballError, ThrottledClient, advertises_large_body, buffer_body,
    extract_body_while_downloading, extract_tarball_buffer, local_file_tarball_path,
    open_local_tarball, read_local_tarball_buffer, starts_with_gzip_magic,
    streaming_extract_semaphore,
};

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
    expected_integrity: Option<&Integrity>,
    package_unpacked_size: Option<usize>,
    download_priority: u64,
    package_id: &str,
    attempt: u32,
    store_dir: &'static StoreDir,
    auth_headers: &AuthHeaders,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
    revision_addressed: bool,
) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
    let download = TarballDownload {
        http_client,
        package_url,
        expected_integrity,
        package_unpacked_size,
        package_id,
        store_dir,
        ignore_file_pattern,
    };
    if let Some(path) = local_file_tarball_path(package_url) {
        return download.fetch_local::<Reporter>(&path, attempt).await;
    }
    let (client, response_head) = crate::archive_request::request_archive::<Reporter>(
        http_client,
        package_url,
        package_id,
        auth_headers,
        download_priority,
        attempt,
        revision_addressed,
    )
    .await?;
    download.extract_response::<Reporter, _>(client, response_head, attempt).await
}

pub(super) struct TarballDownload<'a> {
    pub(super) http_client: &'a ThrottledClient,
    pub(super) package_url: &'a str,
    pub(super) expected_integrity: Option<&'a Integrity>,
    pub(super) package_unpacked_size: Option<usize>,
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
    expected_integrity: Option<&Integrity>,
    package_unpacked_size: Option<usize>,
    package_url: &str,
    package_id: &str,
    attempt: u32,
    store_dir: &'static StoreDir,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
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
        buffer,
        expected_integrity,
        package_unpacked_size,
        package_url,
        store_dir,
        ignore_file_pattern,
    )
    .await
}

/// Pull chunks until the gzip magic is decidable. The selected body path
/// receives the prefix so no bytes are consumed twice.
pub(super) async fn read_gzip_prefix<Body>(
    stream: &mut Body,
    package_url: &str,
) -> Result<(Vec<bytes::Bytes>, usize), TarballError>
where
    Body: Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
{
    let mut prefix: Vec<bytes::Bytes> = Vec::new();
    let mut prefix_len = 0usize;
    while prefix_len < GZIP_MAGIC.len() {
        let Some(chunk) = stream.next().await else { break };
        let chunk = chunk.map_err(|error| fetch_error(package_url, error))?;
        prefix_len += chunk.len();
        prefix.push(chunk);
    }
    Ok((prefix, prefix_len))
}

pub(super) fn fetch_error(package_url: &str, error: reqwest::Error) -> TarballError {
    TarballError::FetchTarball(NetworkError::new(package_url, error))
}

impl TarballDownload<'_> {
    pub(super) async fn fetch_local<Reporter: self::Reporter>(
        self,
        path: &Path,
        attempt: u32,
    ) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
        fetch_local_tarball::<Reporter>(
            path,
            self.expected_integrity,
            self.package_unpacked_size,
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
    ) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
        let expected_size = response.content_length();
        let mut stream = response.bytes_stream();
        let mut progress = BodyProgress::new(expected_size, self.package_id);
        let (prefix, prefix_len) = read_gzip_prefix(&mut stream, self.package_url).await?;
        let is_gzip = starts_with_gzip_magic(&prefix);
        // Retries remain buffered to preserve whole-archive decode diagnostics.
        if is_gzip
            && attempt == 0
            && advertises_large_body(expected_size)
            && let Ok(permit) = streaming_extract_semaphore().try_acquire()
        {
            progress.on_chunks::<Reporter>(&prefix);
            return self
                .stream_body::<Reporter, _, _>(prefix, stream, progress, client, permit)
                .await;
        }
        let buffered = buffer_body::<Reporter, _>(BufferBody {
            stream: &mut stream,
            progress: &mut progress,
            prefix,
            prefix_len,
            expected_size,
            expected_integrity: self.expected_integrity,
            is_gzip,
            package_url: self.package_url,
            http_client: self.http_client,
        })
        .await?;
        self.finish_body::<Reporter, _, _>(buffered, stream, progress, client).await
    }

    pub(super) async fn finish_body<Reporter, Body, Guard>(
        self,
        buffered: Buffered,
        stream: Body,
        progress: BodyProgress<'_>,
        client: Guard,
    ) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError>
    where
        Reporter: self::Reporter,
        Body: Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
    {
        let buffer = match buffered {
            Buffered::Complete(buffer) => buffer,
            Buffered::Overflowed(buffer) => {
                let permit = streaming_extract_semaphore()
                    .acquire()
                    .await
                    .expect("streaming-extract semaphore shouldn't be closed this soon");
                return self
                    .stream_body::<Reporter, _, _>(
                        vec![bytes::Bytes::from(buffer)],
                        stream,
                        progress,
                        client,
                        permit,
                    )
                    .await;
            }
        };
        drop(stream);
        // Release the network slot before the CPU-bound extraction gate.
        // Gating buffering with that smaller semaphore would serialize downloads.
        drop(client);
        extract_tarball_buffer(
            buffer,
            self.expected_integrity,
            self.package_unpacked_size,
            self.package_url,
            self.store_dir,
            self.ignore_file_pattern,
        )
        .await
    }

    pub(super) async fn stream_body<Reporter, Body, Guard>(
        self,
        prefix: Vec<bytes::Bytes>,
        stream: Body,
        mut progress: BodyProgress<'_>,
        client: Guard,
        permit: SemaphorePermit<'static>,
    ) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError>
    where
        Reporter: self::Reporter,
        Body: Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
    {
        extract_body_while_downloading::<Reporter, _, _>(
            prefix,
            stream,
            BodyHasher::new(self.expected_integrity),
            &mut progress,
            client,
            permit,
            self.http_client,
            self.package_url,
            self.store_dir,
            self.ignore_file_pattern,
        )
        .await
    }
}
