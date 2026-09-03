//! Zip-archive fetch and extraction.
//!
//! Runtime archives (Node.js / Bun / Deno) ship as zips rather than
//! gzipped tarballs, so they take a parallel path to the tarball one:
//! same CAS write and progress reporting, different container format.

use super::{
    Arc, Component, Cursor, Duration, HashMap, HttpStatusError, IgnoreEntryFilter, Instant,
    NetworkError, PathBuf, PrefetchedCasPaths, Read, STREAM_ENTRY_BUFFER_MAX, TarballError,
    UNIX_EPOCH, VerifyChecksumError, allocate_tarball_buffer, apply_append_manifest,
    apply_placeholder_manifest, auth_header_for_package_download, emit_progress_found_in_store,
    is_transient_error, load_cached_cas_paths, post_download_semaphore,
    tarball_error_to_request_retry,
};
use pnpm_fs::file_mode;
use pnpm_network::{AuthHeaders, RetryOpts, ThrottledClient};
use pnpm_reporter::{
    FetchingProgressLog, FetchingProgressMessage, LogEvent, LogLevel, ProgressLog, ProgressMessage,
    Reporter, RequestRetryLog,
};
use pnpm_store_dir::{
    CafsFileInfo, FileHash, PackageFilesIndex, SharedReadonlyStoreIndex, SharedVerifiedFilesCache,
    StoreDir, StoreIndexWriter, WriteCasFileFromReaderError, store_index_key,
};
use ssri::Integrity;

/// Walk a zip archive, writing each regular-file entry into the CAFS
/// and returning the `{relative-path → CAFS path}` map plus the
/// per-package [`PackageFilesIndex`] row to hand off to the shared
/// store-index writer. Same contract as
/// [`crate::extract::extract_tarball_entries`], for the
/// `BinaryResolution { archive: zip, .. }` artifacts Node.js / Bun /
/// Deno ship their Windows builds in.
///
/// Directory entries are skipped rather than walked: expanding one
/// would pull in every descendant without consulting
/// `ignore_file_pattern`, leaving the per-file filter no longer
/// authoritative.
///
/// Unix mode comes from the central-directory record, which Windows
/// tooling leaves unpopulated; those entries fall back to `0o644`.
pub(crate) fn extract_zip_entries(
    archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>,
    package_url: &str,
    store_dir: &StoreDir,
    archive_prefix: Option<&str>,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
) -> Result<(HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
    let entry_count = archive.len();
    let mut cas_paths = HashMap::<String, PathBuf>::with_capacity(entry_count);
    let mut pkg_files_idx = PackageFilesIndex {
        manifest: None,
        requires_build: None,
        requires_prepare: None,
        algo: "sha512".to_string(),
        files: HashMap::with_capacity(entry_count),
        side_effects: None,
        remote_side_effects_quarantine: None,
    };

    // Build the `{prefix}/` slice once. Treat `Some("")` as `None`,
    // keeping entry paths verbatim when there is no prefix. The
    // trailing slash anchors the strip so a prefix of `foo` doesn't
    // accidentally consume `foobar/...`.
    let basename_prefix: Option<String> =
        archive_prefix.filter(|prefix| !prefix.is_empty()).map(|prefix| format!("{prefix}/"));

    for i in 0..entry_count {
        let mut entry = archive.by_index(i).map_err(|source| TarballError::ReadZipArchive {
            url: package_url.to_string(),
            source,
        })?;
        // Validate the path *before* the `is_dir()` early-skip so an
        // archive carrying a directory entry like `../evil/` still
        // surfaces [`TarballError::PathTraversal`] rather than being
        // silently dropped. Pacquet wouldn't write that directory
        // either way (only file entries take the CAS write path
        // below), but rejecting outright keeps the "no unsafe entry
        // accepted" contract intact for tooling that inspects the
        // error code.
        let raw_name = entry.name().to_string();
        // [`zip::read::ZipFile::enclosed_name`] returns `None` for
        // absolute paths and any path with a `..` component — a
        // single check covers both forms of path traversal. The
        // returned `PathBuf` has every `.` segment collapsed and is
        // what we use below to build the canonical `cas_paths` /
        // `pkg_files_idx` keys.
        let Some(enclosed) = entry.enclosed_name() else {
            return Err(TarballError::PathTraversal {
                url: package_url.to_string(),
                entry_path: raw_name,
                reason: "zip entry path is absolute or escapes the archive root",
            });
        };
        if entry.is_dir() {
            continue;
        }

        // Rebuild the path as a forward-slash string from the sanitized
        // components, so `.` segments collapse to one canonical key and
        // the ignore filter matches against exactly that key.
        //
        // `enclosed_name` yields only `Normal` components, but a `\`
        // inside one is an ordinary filename character on Unix, so the
        // segments are re-checked by
        // [`crate::extract::archive_entry_segments`], which treats it as
        // a separator the way pnpm does.
        let joined: String = enclosed
            .components()
            .map(|component| match component {
                Component::Normal(name) => name.to_string_lossy().into_owned(),
                _ => unreachable!("enclosed_name returns only Normal components: {:?}", enclosed),
            })
            .collect::<Vec<_>>()
            .join("/");
        let Some(segments) = crate::extract::archive_entry_segments(&joined) else {
            return Err(TarballError::PathTraversal {
                url: package_url.to_string(),
                entry_path: raw_name,
                reason: "zip entry path is absolute or escapes the archive root",
            });
        };
        let normalized = segments.join("/");

        // Strip the archive's top-level basename (`prefix` on
        // `pnpm_lockfile::BinaryResolution`) so the ignore filter
        // sees paths relative to the archive root. If the entry path
        // doesn't start with `{prefix}/` we use the normalized form
        // (a no-op when the entry already lives at the archive root).
        let cleaned = match basename_prefix.as_deref() {
            Some(prefix) => normalized.strip_prefix(prefix).unwrap_or(&normalized).to_string(),
            None => normalized,
        };
        if cleaned.is_empty() {
            // Skip an entry whose name was exactly the prefix
            // directory: no relative payload survives the strip.
            continue;
        }

        if let Some(filter) = ignore_file_pattern
            && filter(&cleaned)
        {
            continue;
        }

        // Central-directory record carries a Unix mode only when
        // the archive was built by a Unix tool; Windows-built
        // archives omit it. Fall back to `0o644` so the executable
        // bit defaults to off. Mask off the high `st_mode` bits
        // (e.g. `0o100000` for a regular file) so `CafsFileInfo.mode`
        // stays permission-only, matching the convention
        // `add_files_from_dir.rs` enforces for tar / on-disk imports.
        let file_mode = entry.unix_mode().unwrap_or(0o644) & 0o777;
        let file_is_executable = file_mode::is_executable(file_mode);
        let declared_size = entry.size();

        let (file_path, file_hash, file_size) = write_zip_entry_to_cas(
            &mut entry,
            declared_size,
            package_url,
            &cleaned,
            store_dir,
            file_is_executable,
        )?;

        let checked_at =
            UNIX_EPOCH.elapsed().ok().and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok());
        let file_attrs = CafsFileInfo {
            digest: format!("{file_hash:x}"),
            mode: file_mode,
            size: file_size,
            checked_at,
        };

        if let Some(previous) = cas_paths.insert(cleaned.clone(), file_path) {
            tracing::warn!(?previous, "Duplication detected. Old entry has been ejected");
        }
        if let Some(previous) = pkg_files_idx.files.insert(cleaned, file_attrs) {
            tracing::warn!(?previous, "Duplication detected. Old entry has been ejected");
        }
    }

    Ok((cas_paths, pkg_files_idx))
}

/// Hash one zip entry into the content-addressed store, holding it in
/// memory only while it is small.
///
/// Nothing in a zip archive bounds an entry's decompressed size:
/// `uncompressed_size` in the central directory is a claim, and the
/// deflate stream behind it keeps producing bytes for as long as it
/// likes — a tar entry, by contrast, is raw bytes the header's size
/// field genuinely delimits. Both branches below therefore read through
/// a [`Read::take`] of the claim and reject an entry whose payload
/// outruns it, rather than growing to whatever it decodes to.
///
/// Entries above [`STREAM_ENTRY_BUFFER_MAX`] go straight into the store
/// with an incremental hash, the same shape
/// [`crate::extract::extract_tarball_entries_streaming`] gives a large
/// tar entry, so a runtime archive's biggest member never has to fit in
/// memory.
///
/// Returns the CAS path, the content hash, and the entry's size.
pub(crate) fn write_zip_entry_to_cas(
    entry: &mut impl Read,
    declared_size: u64,
    package_url: &str,
    entry_path: &str,
    store_dir: &StoreDir,
    executable: bool,
) -> Result<(PathBuf, FileHash, u64), TarballError> {
    let read_error = |source| TarballError::ReadZipEntries {
        url: package_url.to_string(),
        entry_path: entry_path.to_string(),
        source,
    };
    // One byte past the claim is all it takes to tell a truthful entry
    // from one whose payload keeps going.
    let mut bounded = entry.take(declared_size.saturating_add(1));

    if declared_size > STREAM_ENTRY_BUFFER_MAX {
        // `Some(declared_size)` makes the store writer reject a stream
        // that runs short or long before anything is committed to a
        // content-addressed path.
        return store_dir
            .write_cas_file_from_reader(&mut bounded, executable, Some(declared_size))
            .map_err(|error| match error {
                WriteCasFileFromReaderError::Read(error) => read_error(error),
                WriteCasFileFromReaderError::Write(error) => TarballError::WriteCasFile(error),
            });
    }

    let prealloc = declared_size as usize;
    let mut buffer = Vec::new();
    buffer.try_reserve(prealloc).map_err(|err| {
        read_error(std::io::Error::new(
            std::io::ErrorKind::OutOfMemory,
            format!("failed to reserve {prealloc} bytes for zip entry: {err}"),
        ))
    })?;
    bounded.read_to_end(&mut buffer).map_err(read_error)?;
    if buffer.len() as u64 != declared_size {
        return Err(read_error(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "zip entry decompressed to {} bytes where its central-directory record claims {declared_size}",
                buffer.len(),
            ),
        )));
    }

    let (file_path, file_hash) =
        store_dir.write_cas_file(&buffer, executable).map_err(TarballError::WriteCasFile)?;
    Ok((file_path, file_hash, declared_size))
}

/// Run one full zip-archive fetch attempt: hit the network, drain the
/// body into RAM, verify the integrity hash, then walk the zip and
/// extract every file entry into the CAFS. Mirrors
/// [`crate::download::fetch_and_extract_once`] one-for-one (same network permit
/// shape, same post-download semaphore gate, same retry-friendly
/// errors) — only the `spawn_blocking` body differs: integrity check
/// then [`extract_zip_entries`] instead of the gzip + tar path.
///
/// Writes directly into the CAS via [`StoreDir::write_cas_file`]
/// rather than extracting to a temp dir and importing each file.
// 8 arguments — over the default clippy threshold, but each is
// distinct (see the matching note on `fetch_and_extract_zip_with_retry`).
#[expect(
    clippy::too_many_arguments,
    reason = "the parameters are independent install-scoped inputs; bundling them into a struct only moves the same fields into a wrapper"
)]
pub(crate) async fn fetch_and_extract_zip_once<Reporter: self::Reporter>(
    http_client: &ThrottledClient,
    package_url: &str,
    package_integrity: &Integrity,
    package_id: &str,
    attempt: u32,
    store_dir: &'static StoreDir,
    auth_headers: &AuthHeaders,
    archive_prefix: Option<&str>,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
) -> Result<(HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
    let network_error =
        |error| TarballError::FetchTarball(NetworkError { url: package_url.to_string(), error });

    // The route policy decides whether this origin may be reached at all,
    // at the fetch rather than when the request that named it was read.
    if !auth_headers.allows_fetch(package_url) {
        return Err(TarballError::OffAllowlist {
            url: pnpm_network::redact_url_credentials(package_url),
        });
    }
    let client = http_client.acquire_for_url(package_url).await;

    let mut request = client.get(package_url);
    // Match the tarball download path: resolve the per-URL auth
    // header and attach it. Runtime artifacts (Node.js, Bun, Deno)
    // are typically downloaded from public hosts that don't require
    // auth, but a self-hosted mirror behind a token-protected proxy
    // would 401 without this. Keeps parity with pnpm's binary
    // fetcher which goes through the same `fetchFromRegistry` /
    // auth-header plumbing.
    if let Some(value) = auth_header_for_package_download(auth_headers, package_url, package_id) {
        request = request.header("authorization", value);
    }

    let send_result = request.send().await;
    let size = send_result.as_ref().ok().and_then(reqwest::Response::content_length);
    Reporter::emit(&LogEvent::FetchingProgress(FetchingProgressLog {
        level: LogLevel::Debug,
        message: FetchingProgressMessage::Started {
            attempt: attempt + 1,
            package_id: package_id.to_owned(),
            size,
        },
    }));
    let response_head = send_result.map_err(network_error)?;

    let status = response_head.status();
    if !status.is_success() {
        const DRAIN_CAP: u64 = 64 * 1024;
        if response_head.content_length().is_some_and(|len| len <= DRAIN_CAP) {
            let _ = response_head.bytes().await;
        }
        return Err(TarballError::HttpStatus(HttpStatusError {
            url: package_url.to_string(),
            status: status.as_u16(),
        }));
    }

    let expected_size = response_head.content_length();

    let buffer = {
        use futures_util::StreamExt;
        let mut buf = allocate_tarball_buffer(expected_size, package_url)?;
        let mut stream = response_head.bytes_stream();

        const BIG_TARBALL_SIZE: u64 = 5 * 1024 * 1024;
        const IN_PROGRESS_THROTTLE: Duration = Duration::from_millis(500);
        let emit_progress = expected_size.is_some_and(|size| size >= BIG_TARBALL_SIZE);
        let mut last_emit: Option<Instant> = None;
        let mut last_emitted_downloaded: u64 = 0;
        let mut downloaded: u64 = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(network_error)?;
            buf.extend_from_slice(&chunk);
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            let throttle_ready =
                last_emit.is_none_or(|instant| instant.elapsed() >= IN_PROGRESS_THROTTLE);
            if emit_progress && throttle_ready {
                Reporter::emit(&LogEvent::FetchingProgress(FetchingProgressLog {
                    level: LogLevel::Debug,
                    message: FetchingProgressMessage::InProgress {
                        downloaded,
                        package_id: package_id.to_owned(),
                    },
                }));
                last_emit = Some(Instant::now());
                last_emitted_downloaded = downloaded;
            }
        }
        if emit_progress && downloaded != last_emitted_downloaded {
            Reporter::emit(&LogEvent::FetchingProgress(FetchingProgressLog {
                level: LogLevel::Debug,
                message: FetchingProgressMessage::InProgress {
                    downloaded,
                    package_id: package_id.to_owned(),
                },
            }));
        }
        buf
    };
    drop(client);

    let _post_download_permit = post_download_semaphore()
        .acquire()
        .await
        .expect("post-download semaphore shouldn't be closed this soon");

    tracing::info!(target: "pacquet::download", ?package_url, "Download completed");

    let package_integrity = package_integrity.clone();
    let package_url_owned = package_url.to_string();
    let archive_prefix_owned: Option<String> = archive_prefix.map(str::to_string);
    let result = tokio::task::spawn_blocking(
        move || -> Result<(HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
            package_integrity.check(&buffer).map_err(|error| {
                TarballError::Checksum(VerifyChecksumError {
                    url: package_url_owned.clone(),
                    error,
                })
            })?;

            // Open the archive in a scope so the buffer + ZipArchive
            // are released before we return — large runtime archives
            // (Node.js for Windows is ~30 MB) keep the buffer alive
            // through the whole read otherwise.
            let (cas_paths, pkg_files_idx) = {
                let cursor = Cursor::new(buffer);
                let mut archive = zip::ZipArchive::new(cursor).map_err(|source| {
                    TarballError::ReadZipArchive { url: package_url_owned.clone(), source }
                })?;
                extract_zip_entries(
                    &mut archive,
                    &package_url_owned,
                    store_dir,
                    archive_prefix_owned.as_deref(),
                    ignore_file_pattern.as_deref(),
                )?
            };
            Ok((cas_paths, pkg_files_idx))
        },
    )
    .await
    .map_err(TarballError::TaskJoin)??;

    tracing::info!(target: "pacquet::download", ?package_url, "Checksum verified");

    Ok(result)
}

/// Run [`fetch_and_extract_zip_once`] under pnpm's retry policy.
/// Same shape as [`crate::download::fetch_and_extract_with_retry`]: HTTP 401 / 403 /
/// 404 fail fast, every other error retries with exponential
/// backoff until [`RetryOpts::retries`] is exhausted. On success
/// emits `pnpm:progress fetched` once per (resolved) package, same
/// as the tarball path.
// 10 arguments — over the default clippy threshold for the same
// reason `fetch_and_extract_with_retry` is: each is distinct, and
// bundling into a struct would just push the same fields into a
// wrapper.
#[expect(clippy::too_many_arguments, reason = "arg count is fixed by the fetcher signature")]
pub(crate) async fn fetch_and_extract_zip_with_retry<Reporter: self::Reporter>(
    http_client: &ThrottledClient,
    package_url: &str,
    package_integrity: &Integrity,
    package_id: &str,
    requester: &str,
    store_dir: &'static StoreDir,
    retry_opts: RetryOpts,
    auth_headers: &AuthHeaders,
    archive_prefix: Option<&str>,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
) -> Result<(HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
    let mut attempt: u32 = 0;
    loop {
        let result = fetch_and_extract_zip_once::<Reporter>(
            http_client,
            package_url,
            package_integrity,
            package_id,
            attempt,
            store_dir,
            auth_headers,
            archive_prefix,
            ignore_file_pattern.clone(),
        )
        .await;
        match result {
            Ok(value) => {
                Reporter::emit(&LogEvent::Progress(ProgressLog {
                    level: LogLevel::Debug,
                    message: ProgressMessage::Fetched {
                        package_id: package_id.to_owned(),
                        requester: requester.to_owned(),
                    },
                }));
                return Ok(value);
            }
            Err(err) if !is_transient_error(&err) => return Err(err),
            Err(err) if attempt >= retry_opts.retries => {
                tracing::warn!(
                    target: "pacquet::download",
                    ?package_url,
                    attempts = attempt + 1,
                    ?err,
                    "Zip archive fetch retry budget exhausted",
                );
                return Err(err);
            }
            Err(err) => {
                let delay = retry_opts.delay_for(attempt);
                tracing::warn!(
                    target: "pacquet::download",
                    ?package_url,
                    attempt = attempt + 1,
                    max_attempts = retry_opts.retries + 1,
                    ?delay,
                    ?err,
                    "Zip archive fetch failed; retrying after backoff",
                );
                Reporter::emit(&LogEvent::RequestRetry(RequestRetryLog {
                    level: LogLevel::Debug,
                    attempt: attempt + 1,
                    error: tarball_error_to_request_retry(&err),
                    max_retries: retry_opts.retries,
                    method: "GET".to_string(),
                    timeout: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    url: package_url.to_string(),
                }));
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }
    }
}

/// Counterpart to [`crate::download::DownloadTarballToStore`] for zip-archive binary
/// resolutions: the zip flow downloads the body, verifies the
/// integrity hash, then walks zip entries and writes each to the CAFS
/// — with the `prefix` field stripped from each entry path before the
/// ignore filter and CAS write so the runtime's top-level
/// `node-vX.Y.Z-<platform>-<arch>/` directory doesn't leak into
/// downstream consumers' paths.
///
/// The store-index lookup, prefetch cache reuse, and store-index
/// writer queueing match [`crate::download::DownloadTarballToStore`] — runtime
/// artifacts share the same `index.db` schema as ordinary npm
/// packages.
#[must_use]
pub struct DownloadZipArchiveToStore<'a> {
    pub http_client: &'a ThrottledClient,
    pub store_dir: &'static StoreDir,
    pub store_index: Option<SharedReadonlyStoreIndex>,
    pub store_index_writer: Option<Arc<StoreIndexWriter>>,
    pub verify_store_integrity: bool,
    /// See [`crate::download::DownloadTarballToStore::strict_store_pkg_content_check`].
    pub strict_store_pkg_content_check: bool,
    pub verified_files_cache: SharedVerifiedFilesCache,
    pub package_integrity: &'a Integrity,
    pub package_url: &'a str,
    pub package_id: &'a str,
    pub requester: &'a str,
    pub prefetched_cas_paths: Option<&'a PrefetchedCasPaths>,
    pub retry_opts: RetryOpts,
    /// Auth headers resolved at install start. The zip pipeline
    /// applies the per-URL match the same way the tarball pipeline
    /// does (`AuthHeaders::for_url`), so a runtime archive hosted
    /// behind a token-protected proxy still authenticates correctly.
    pub auth_headers: &'a AuthHeaders,
    /// Basename of the archive's top-level directory, mirroring the
    /// `prefix` field on `pnpm_lockfile::BinaryResolution`. The
    /// zip extractor strips `{prefix}/` from each entry path before
    /// the ignore-filter check and the CAS write, so downstream
    /// consumers see paths relative to the package root rather than
    /// the runtime-version-stamped wrapper directory.
    pub archive_prefix: Option<&'a str>,
    /// See [`crate::download::DownloadTarballToStore::ignore_file_pattern`] — the
    /// per-fetch archive filter is shared by both archive types.
    pub ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
    /// See [`crate::download::DownloadTarballToStore::offline`]. Same semantics for
    /// the zip-archive path: when both cache lookups miss and
    /// `offline` is `true`, the fetcher fails with
    /// [`TarballError::NoOfflineTarball`] rather than hitting the
    /// network.
    pub offline: bool,
    /// Synthesized `package.json` to fold into the extracted archive.
    /// See [`crate::download::DownloadTarballToStore::append_manifest`] — same
    /// `appendManifest` semantics for the zip path, used for the
    /// runtime archives (e.g. Deno / Bun) that arrive as zips.
    pub append_manifest: Option<&'a [u8]>,
}

impl DownloadZipArchiveToStore<'_> {
    /// Execute the subroutine without an in-memory cache. Mirrors
    /// [`crate::download::DownloadTarballToStore::run_without_mem_cache`] — same
    /// prefetch-cas-paths reuse, same SQLite-index lookup, same
    /// store-index writer queue — only the network and extract
    /// path differs (zip instead of gzip + tar).
    pub async fn run_without_mem_cache<Reporter: self::Reporter>(
        &self,
    ) -> Result<HashMap<String, PathBuf>, TarballError> {
        let &DownloadZipArchiveToStore {
            http_client,
            store_dir,
            package_integrity,
            package_url,
            package_id,
            requester,
            verify_store_integrity,
            strict_store_pkg_content_check,
            prefetched_cas_paths,
            retry_opts,
            auth_headers,
            archive_prefix,
            append_manifest,
            ..
        } = self;
        let store_index = self.store_index.clone();
        let store_index_writer = self.store_index_writer.clone();
        let verified_files_cache = Arc::clone(&self.verified_files_cache);
        // See the matching note in
        // [`crate::download::DownloadTarballToStore::run_without_mem_cache`]: the
        // Arc-wrapped filter can't ride along in the deref pattern,
        // so clone it out by hand.
        let ignore_file_pattern = self.ignore_file_pattern.clone();

        let cache_key = store_index_key(&package_integrity.to_string(), package_id);
        if let Some(prefetched) = prefetched_cas_paths
            && let Some(cas_paths) = prefetched.get(&cache_key)
        {
            tracing::info!(
                target: "pacquet::download",
                ?package_url,
                ?package_id,
                "Reusing prefetched CAFS entry — skipping zip download",
            );
            emit_progress_found_in_store::<Reporter>(package_id, requester, None);
            return Ok((**cas_paths).clone());
        }
        let cached = load_cached_cas_paths::<Reporter>(
            store_index,
            store_dir,
            cache_key,
            verify_store_integrity,
            strict_store_pkg_content_check,
            verified_files_cache,
        )
        .await?;
        if let Some(cas_paths) = cached {
            tracing::info!(target: "pacquet::download", ?package_url, ?package_id, "Reusing cached CAFS entry — skipping zip download");
            emit_progress_found_in_store::<Reporter>(package_id, requester, None);
            return Ok(cas_paths);
        }

        // Offline-mode gate (zip archive). Same shape as the tarball
        // path above — see the matching comment there for the
        // rationale.
        if self.offline {
            tracing::warn!(
                target: "pacquet::download",
                ?package_url,
                ?package_id,
                "offline mode: zip archive missing from local store; refusing network fetch",
            );
            return Err(TarballError::NoOfflineTarball {
                package_id: package_id.to_string(),
                url: package_url.to_string(),
            });
        }

        tracing::info!(target: "pacquet::download", ?package_url, "New cache (zip)");

        let (mut cas_paths, mut pkg_files_idx) = fetch_and_extract_zip_with_retry::<Reporter>(
            http_client,
            package_url,
            package_integrity,
            package_id,
            requester,
            store_dir,
            retry_opts,
            auth_headers,
            archive_prefix,
            ignore_file_pattern,
        )
        .await?;

        // Fold the synthesized runtime `package.json` into the row before
        // it is persisted, so warm reinstalls (which read the row) get it.
        if let Some(manifest_bytes) = append_manifest {
            apply_append_manifest(store_dir, manifest_bytes, &mut cas_paths, &mut pkg_files_idx)?;
        }
        apply_placeholder_manifest(store_dir, &mut cas_paths, &mut pkg_files_idx)?;

        let index_key = store_index_key(&package_integrity.to_string(), package_id);
        if let Some(writer) = store_index_writer {
            writer.queue(index_key, pkg_files_idx);
        } else {
            tracing::warn!(
                target: "pacquet::download",
                ?index_key,
                "no shared store-index writer; skipping index row for this zip archive",
            );
        }

        Ok(cas_paths)
    }
}
