use std::{
    collections::HashMap,
    io::{Cursor, Read},
    path::{Component, PathBuf},
    sync::{Arc, OnceLock},
    time::{Duration, Instant, UNIX_EPOCH},
};

use dashmap::{DashMap, DashSet};
use derive_more::{Display, Error, From};
use miette::Diagnostic;
use pacquet_fs::file_mode;
pub use pacquet_network::RetryOpts;
use pacquet_network::{AuthHeaders, ThrottledClient, UNPRIORITIZED};
use pacquet_reporter::{
    FetchingProgressLog, FetchingProgressMessage, LogEvent, LogLevel, ProgressLog, ProgressMessage,
    Reporter, RequestRetryError, RequestRetryLog,
};
use pacquet_store_dir::{
    CafsFileInfo, PackageFilesIndex, SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreDir,
    StoreIndexError, StoreIndexWriter, WriteCasFileError, store_index_key,
};
use pipe_trait::Pipe;
use rayon::prelude::*;
use ssri::{Algorithm, Integrity, IntegrityOpts};
use tar::Archive;
use tokio::sync::{Notify, RwLock, Semaphore};
use tracing::instrument;
use zune_inflate::{DeflateDecoder, DeflateOptions, errors::InflateDecodeErrors};

/// Cap on concurrent post-download tarball work (SHA-512 of the whole
/// tarball + gzip inflate + per-file SHA-512 + CAFS writes). The body is
/// CPU-bound with some blocking FS I/O, and putting it on
/// `tokio::task::spawn_blocking` makes the default 512-thread blocking
/// pool available — but async fan-out across `try_join_all` routinely
/// fires hundreds of these at once on a 1352-snapshot install, which
/// thrashes small CI runners. Past "Download completed" a 2-CPU GitHub
/// Actions runner wedged between decompress-close and `Checksum verified`
/// on [#269] until the step timeout. `num_cpus * 2` (floor 4) keeps enough
/// work in flight to overlap per-file FS writes with SHA on another task
/// without oversubscribing the cores.
///
/// [#269]: https://github.com/pnpm/pacquet/pull/269
fn post_download_semaphore() -> &'static Semaphore {
    static SEM: OnceLock<Semaphore> = OnceLock::new();
    SEM.get_or_init(|| Semaphore::new(num_cpus::get().saturating_mul(2).max(4)))
}

/// Dedicated rayon pool for the per-file CAS-write phase of extraction
/// ([`extract_tarball_entries`]).
///
/// Kept separate from rayon's global pool on purpose. The install
/// pipeline overlaps tarball extraction with linking each package into
/// `node_modules`, and the linker runs its per-package work through
/// `rayon::join` / `par_iter` on the *global* pool. If extraction also
/// used the global pool, a burst of extraction work (hundreds of
/// tarballs finishing downloads at once) would queue ahead of the
/// linker's jobs and stall linking for seconds. Routing the CAS writes
/// through their own pool lets the two phases run concurrently without
/// one starving the other.
///
/// Sized to the core count: the work is CPU-bound (SHA-512 + CAFS
/// write), so more threads than cores only adds scheduling contention.
/// Returns `None` if the pool can't be built, in which case the caller
/// falls back to the global pool.
fn cas_write_pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(num_cpus::get().max(1))
            .thread_name(|index| format!("cas-write-{index}"))
            .build()
            .map_err(|error| {
                tracing::warn!(
                    target: "pacquet::download",
                    ?error,
                    "failed to build the dedicated CAS-write pool; falling back to the global rayon pool",
                );
            })
            .ok()
    })
    .as_ref()
}

/// Reqwest's own [`std::fmt::Display`] for a request-stage failure renders as
/// `error sending request for url (URL): <inner>` only if it can find
/// an inner source, and on some failure modes (e.g. the request was
/// dropped before a connect was attempted) `inner` is `None` —
/// leaving the user with the truly opaque `error sending request for
/// url (URL)` and no clue about what actually failed.
///
/// `walk_reqwest_chain` walks `error.source()` itself and joins every
/// stage's `Display` with `: ` so the rendered `NetworkError` always
/// carries the leaf reason (e.g. `Connection refused (os error 61)`,
/// `tls handshake eof`, `dns error: failed to lookup address`),
/// regardless of which intermediate `reqwest` / `hyper` / `io::Error`
/// happens to elide it.
fn walk_reqwest_chain(error: &reqwest::Error) -> String {
    let mut out = error.to_string();
    let mut error: &dyn std::error::Error = error;
    while let Some(src) = error.source() {
        let frame = src.to_string();
        // Skip empty or duplicate frames — hyper occasionally repeats
        // the same message across two layers, and reqwest sometimes
        // already includes the inner string in its top-level Display.
        if !frame.is_empty() && !out.ends_with(&frame) {
            out.push_str(": ");
            out.push_str(&frame);
        }
        error = src;
    }
    out
}

#[derive(Debug, Display, Error, Diagnostic)]
#[display("Failed to fetch {url}: {}", walk_reqwest_chain(error))]
pub struct NetworkError {
    pub url: String,
    /// Marked `#[error(source)]` so miette can also walk the chain on
    /// its own (some renderers prefer the structured form). The
    /// flattened string in `Display` is for the default miette report
    /// where the user just sees one line per wrapper.
    #[error(source)]
    pub error: reqwest::Error,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[display("Tarball server returned HTTP {status} for {url}")]
pub struct HttpStatusError {
    pub url: String,
    pub status: u16,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[display("Failed to verify the integrity of {url}: {error}")]
pub struct VerifyChecksumError {
    pub url: String,
    #[error(source)]
    pub error: ssri::Error,
}

#[derive(Debug, Display, Error, Diagnostic, From)]
#[non_exhaustive]
pub enum TarballError {
    #[diagnostic(code(pacquet_tarball::fetch_tarball))]
    FetchTarball(NetworkError),

    #[diagnostic(code(pacquet_tarball::http_status))]
    HttpStatus(HttpStatusError),

    #[from(ignore)]
    #[diagnostic(code(pacquet_tarball::io_error))]
    ReadTarballEntries(std::io::Error),

    #[diagnostic(
        code(pacquet_tarball::verify_checksum_error),
        help(
            "The downloaded tarball does not match the integrity recorded in the lockfile. If you trust the new content (legitimate republish, or stale local metadata cache), run `pnpm install --update-checksums` (or `pacquet install --update-checksums`). Otherwise treat this as a potential supply-chain issue and verify the new content first."
        )
    )]
    Checksum(VerifyChecksumError),

    #[from(ignore)]
    #[display("Failed to decode gzip: {_0}")]
    #[diagnostic(code(pacquet_tarball::decode_gzip))]
    DecodeGzip(InflateDecodeErrors),

    #[from(ignore)]
    #[display("Failed to write cafs: {_0}")]
    #[diagnostic(transparent)]
    WriteCasFile(WriteCasFileError),

    #[from(ignore)]
    #[display("Failed to write store index (SQLite index): {_0}")]
    #[diagnostic(transparent)]
    WriteStoreIndex(StoreIndexError),

    #[from(ignore)]
    #[diagnostic(code(pacquet_tarball::task_join_error))]
    TaskJoin(tokio::task::JoinError),

    #[from(ignore)]
    #[display(
        "Archive at {url} advertised a Content-Length of {advertised_size} bytes, which exceeds what pacquet can allocate (either larger than `usize::MAX` on this target or memory pressure prevented a one-shot reservation)"
    )]
    #[diagnostic(code(pacquet_tarball::tarball_too_large))]
    TarballTooLarge { url: String, advertised_size: u64 },

    /// A concurrent request for the same tarball URL went through
    /// `run_with_mem_cache`, drove the network fetch, and failed.
    /// This task was parked on the shared `Notify` waiting for the
    /// download; on wake it sees [`CacheValue::Failed`] and surfaces
    /// this variant. The owner's original error stays with the
    /// owner (it can't be cloned past `reqwest::Error`).
    #[from(ignore)]
    #[display(
        "A concurrent fetch for {url} failed; this request waited on the shared mem cache and inherits the failure"
    )]
    #[diagnostic(code(pacquet_tarball::sibling_fetch_failed))]
    SiblingFetchFailed { url: String },

    /// Path-traversal rejection on a zip entry. Mirrors upstream's
    /// `PATH_TRAVERSAL` error in
    /// [`fetching/binary-fetcher/src/index.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/fetching/binary-fetcher/src/index.ts):
    /// any entry whose path is absolute or whose normalized form
    /// would land outside the target directory is rejected before any
    /// bytes are written to the CAS.
    #[from(ignore)]
    #[display("Refusing to extract zip entry {entry_path:?} from {url} — {reason}")]
    #[diagnostic(code(pacquet_tarball::path_traversal))]
    PathTraversal { url: String, entry_path: String, reason: &'static str },

    /// Zip-archive parse / read error. Wraps the underlying `zip`
    /// crate error verbatim; pacquet does not interpret the failure
    /// mode beyond surfacing the entry path that triggered it.
    #[from(ignore)]
    #[display("Failed to read zip archive {url}: {source}")]
    #[diagnostic(code(pacquet_tarball::read_zip))]
    ReadZipArchive {
        url: String,
        #[error(source)]
        source: zip::result::ZipError,
    },

    /// Per-entry I/O failure during zip extraction — `try_reserve`
    /// for the entry's payload, the body read, or any other
    /// [`std::io::Error`] surfaced from the zip iterator. Carries
    /// the archive URL and the entry path that triggered the
    /// failure so a corrupt archive is diagnosable from the user-
    /// facing message; the underlying [`std::io::Error`] is
    /// exposed as `source` for miette / `Error::source` walkers.
    /// Kept separate from [`TarballError::ReadTarballEntries`] so
    /// the retry-classification path emits `ERR_PACQUET_ZIP`
    /// rather than the tar-specific `ERR_PACQUET_TARBALL_TAR`.
    #[from(ignore)]
    #[display("Failed to read zip entry {entry_path:?} from {url}: {source}")]
    #[diagnostic(code(pacquet_tarball::read_zip_entry))]
    ReadZipEntries {
        url: String,
        entry_path: String,
        #[error(source)]
        source: std::io::Error,
    },

    /// `offline: true` was set and the package's tarball wasn't
    /// found in the local store. Pacquet refuses to fetch the
    /// network. Upstream pnpm's `--offline` only gates the metadata
    /// fetch in [`pickPackage`](https://github.com/pnpm/pnpm/blob/94240bc046/resolving/npm-resolver/src/pickPackage.ts);
    /// pacquet has no metadata fetch on the frozen-install path, so
    /// the same flag's most useful effect lands here: surface a
    /// clear "the snapshot isn't cached" error rather than letting
    /// the underlying network refusal propagate.
    ///
    /// `ERR_PACQUET_NO_OFFLINE_TARBALL` is a pacquet-specific code
    /// (upstream has no exact equivalent); the message shape
    /// follows upstream's
    /// [`ERR_PNPM_NO_OFFLINE_META`](https://github.com/pnpm/pnpm/blob/94240bc046/resolving/npm-resolver/src/pickPackage.ts)
    /// — "Failed to resolve `<pkg>` in package mirror `<dir>`".
    #[from(ignore)]
    #[display(
        "Failed to fetch tarball for {package_id} from {url} in offline mode: snapshot not present in local store"
    )]
    #[diagnostic(
        code(ERR_PACQUET_NO_OFFLINE_TARBALL),
        help(
            "Drop `--offline` (or `offline=true` in pnpm-workspace.yaml) or run an online install first to populate the store."
        )
    )]
    NoOfflineTarball { package_id: String, url: String },
}

/// Per-package callback that decides whether a given archive entry
/// (path relative to the archive's top-level directory, after the
/// `prefix` strip on zip archives, after the `package/` strip on
/// npm tarballs) should be excluded from the CAS write.
///
/// Mirrors upstream's `ignoreFilePattern` / `archiveFilters` regex
/// at <https://github.com/pnpm/pnpm/blob/94240bc046/fetching/binary-fetcher/src/index.ts>.
/// Pacquet uses a callback rather than a regex so the caller can
/// hand-code the filter without pulling a regex engine into
/// `pacquet-tarball`; the canonical Node-runtime filter lives at
/// the install-dispatch site (Slice D) where it's constructed once
/// per fetch.
///
/// The callback receives the *cleaned* path (post-prefix strip,
/// `to_string_lossy()` already applied), so its inputs are stable
/// strings matching what pnpm's regex sees upstream.
pub type IgnoreEntryFilter = dyn Fn(&str) -> bool + Send + Sync;

/// Value of the cache.
#[derive(Debug, Clone)]
pub enum CacheValue {
    /// The package is being processed.
    InProgress(Arc<Notify>),
    /// The package is saved.
    Available(Arc<HashMap<String, PathBuf>>),
    /// The owning fetch failed; concurrent waiters wake up to this
    /// instead of `Available` and surface a sibling-fetch-failed
    /// error rather than blocking on the `Notify` forever. The
    /// originating `TarballError` cannot be cloned past the owner
    /// (it's wrapped in `reqwest::Error` / IO chains that aren't
    /// `Clone`), so waiters return their own variant — see
    /// [`TarballError::SiblingFetchFailed`].
    Failed,
}

/// Internal in-memory cache of tarballs.
///
/// The key of this hashmap is the url of each tarball.
pub type MemCache = DashMap<String, Arc<RwLock<CacheValue>>>;

/// Install-scoped set of store-index cache keys
/// (`store_index_key(integrity, pkg_id)`) whose package status
/// (`fetched` or `found_in_store`) has already been emitted during this
/// install.
///
/// The resolve-time prefetcher emits download/cache-hit progress as soon
/// as it knows the outcome, then records the key here. The later
/// virtual-store warm batch still emits `resolved`, but skips the second
/// package status for recorded keys, so progress is timely without
/// double-counting. See <https://github.com/pnpm/pnpm/issues/12235>.
pub type ReportedProgressKeys = DashSet<String>;

/// Shared handle to a [`ReportedProgressKeys`] set, allocated once per
/// install and shared between early fetchers and the later install-pass
/// reporter.
pub type SharedReportedProgressKeys = Arc<ReportedProgressKeys>;

/// Build the buffer that the tarball body streams into, pre-sized
/// from the response's advertised `Content-Length` when it fits and
/// can actually be reserved without allocation failure.
///
/// `Content-Length` is untrusted input — a malicious or broken
/// registry could advertise `u64::MAX`, which would crash the
/// process if we passed it directly to `Vec::with_capacity`. Two
/// guards:
///
/// 1. `usize::try_from(size)` — on 32-bit targets a `u64` header
///    value may exceed `usize::MAX`; on 64-bit the two are the
///    same width but the conversion is cheap anyway.
/// 2. `Vec::try_reserve_exact(cap)` — if the allocator refuses
///    (legitimate OOM, or because `cap` is absurdly large relative
///    to available RAM), we surface `TarballTooLarge` instead of
///    aborting via the infallible `with_capacity` path.
///
/// When `content_length` is absent the response uses chunked
/// transfer encoding and we can't pre-size; return an empty
/// growable `Vec` and let the stream loop extend it.
fn allocate_tarball_buffer(
    content_length: Option<u64>,
    url: &str,
) -> Result<Vec<u8>, TarballError> {
    let Some(size) = content_length else {
        return Ok(Vec::new());
    };

    let too_large =
        || TarballError::TarballTooLarge { url: url.to_string(), advertised_size: size };

    let capacity = usize::try_from(size).map_err(|_| too_large())?;
    let mut buf = Vec::new();
    buf.try_reserve_exact(capacity).map_err(|_| too_large())?;
    Ok(buf)
}

#[instrument(skip(gz_data), fields(gz_data_len = gz_data.len()))]
fn decompress_gzip(gz_data: &[u8], unpacked_size: Option<usize>) -> Result<Vec<u8>, TarballError> {
    let mut options = DeflateOptions::default().set_confirm_checksum(false);

    if let Some(size) = unpacked_size {
        options = options.set_size_hint(size);
    }

    DeflateDecoder::new_with_options(gz_data, options)
        .decode_gzip()
        .map_err(TarballError::DecodeGzip)
}

/// Mirror of pnpm's `normalizeBundledManifest` at
/// <https://github.com/pnpm/pnpm/blob/4750fd370c/store/cafs/src/normalizeBundledManifest.ts>:
/// pick the subset of `package.json` fields that downstream code
/// (bin linking, dependency resolution, build-script detection)
/// actually reads, and discard the rest. Two reasons for the
/// subset: (1) `index.db` row size on disk — a full manifest can be
/// tens of KB; (2) msgpackr-records slot pressure on the encoder
/// (record slots top out at `0x7f`, see [`pacquet_store_dir::EncodeError::OutOfRecordSlots`]).
///
/// `scripts` is further narrowed to just the three lifecycle hooks
/// pnpm actually executes — `preinstall`, `install`, `postinstall`.
/// Other script keys (test, build, lint, etc.) are dev ergonomics
/// that the installer never invokes.
///
/// Returns `None` when nothing survives the pick. In practice this
/// only happens for inputs that aren't a JSON object at all (the
/// type guard at the top of the function), or for objects whose
/// every field is either absent or `null`. A real `package.json`
/// from an npm tarball always carries at least `name` and `version`
/// (both kept by the pick), so the typical npm-published manifest
/// surfaces as `Some(...)`. Matches upstream's
/// `if (!result && !scripts) return undefined` shape: empty inputs
/// degrade to `None` rather than a `Some(Object({}))` that would
/// round-trip as a zero-field record def.
fn normalize_bundled_manifest(value: &serde_json::Value) -> Option<serde_json::Value> {
    /// Fields kept verbatim from the source manifest.
    ///
    /// Order matters for the on-wire byte sequence — msgpackr emits
    /// fields in JS object insertion order, and pacquet's encoder
    /// follows the [`serde_json::Map`] iteration order — but it
    /// does *not* matter for property-access correctness on the
    /// pnpm side. The order below mirrors pnpm's
    /// `BUNDLED_MANIFEST_FIELDS` array so a side-by-side byte diff
    /// against a pnpm-written row is shallower.
    const BUNDLED_MANIFEST_FIELDS: &[&str] = &[
        "bin",
        "bundledDependencies",
        "bundleDependencies",
        "cpu",
        "dependencies",
        "devDependencies",
        "directories",
        "engines",
        "libc",
        "name",
        "optionalDependencies",
        "os",
        "peerDependencies",
        "peerDependenciesMeta",
    ];
    const LIFECYCLE_SCRIPTS: &[&str] = &["preinstall", "install", "postinstall"];

    let serde_json::Value::Object(map) = value else { return None };
    let mut picked = serde_json::Map::new();

    // pnpm emits `version` first regardless of whether it was first
    // in the source object. Keep the same ordering so a byte diff
    // against a pnpm-written row stays minimal. Version normalization
    // via `semver.clean(...)` (pnpm only loose-cleans for the bundled
    // row, not for resolution) is intentionally skipped: the inputs
    // from a real npm tarball are already semver-clean in practice,
    // and pulling `node-semver` into `pacquet-tarball` purely for
    // this normalization would carry more risk than the deviation it
    // closes.
    if let Some(v) = map.get("version")
        && !v.is_null()
    {
        picked.insert("version".to_string(), v.clone());
    }

    for &key in BUNDLED_MANIFEST_FIELDS {
        if let Some(v) = map.get(key)
            && !v.is_null()
        {
            picked.insert(key.to_string(), v.clone());
        }
    }

    if let Some(serde_json::Value::Object(scripts)) = map.get("scripts") {
        let mut sub = serde_json::Map::new();
        for &key in LIFECYCLE_SCRIPTS {
            if let Some(s) = scripts.get(key)
                && !s.is_null()
            {
                sub.insert(key.to_string(), s.clone());
            }
        }
        if !sub.is_empty() {
            picked.insert("scripts".to_string(), serde_json::Value::Object(sub));
        }
    }

    if picked.is_empty() { None } else { Some(serde_json::Value::Object(picked)) }
}

/// One regular-file tar entry whose path has been validated and
/// cleaned, paired with a borrow of its payload inside the decompressed
/// archive buffer. Collected serially while walking the tar stream, then
/// hashed and written to the CAFS — serially or across the rayon pool —
/// in [`write_cas_entry`].
struct PendingFile<'a> {
    cleaned_path: String,
    data: &'a [u8],
    executable: bool,
    mode: u32,
    size: u64,
}

/// Hash one [`PendingFile`] into the content-addressed store and build
/// its [`CafsFileInfo`] index row. Pure given the inputs and the store
/// dir's content-addressed layout, so it is safe to run concurrently
/// across entries of the same tarball.
fn write_cas_entry(
    store_dir: &StoreDir,
    file: &PendingFile<'_>,
) -> Result<(String, PathBuf, CafsFileInfo), TarballError> {
    let (file_path, file_hash) =
        store_dir.write_cas_file(file.data, file.executable).map_err(TarballError::WriteCasFile)?;
    // `as_millis()` returns `u128`; narrow to `u64` to match the store
    // index schema (see `CafsFileInfo::checked_at`). Drop the timestamp
    // if the clock reports something unrepresentable — `checkedAt` is
    // optional and pnpm tolerates `None`.
    let checked_at =
        UNIX_EPOCH.elapsed().ok().and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok());
    let info = CafsFileInfo {
        digest: format!("{file_hash:x}"),
        mode: file.mode,
        size: file.size,
        checked_at,
    };
    Ok((file.cleaned_path.clone(), file_path, info))
}

/// Walk decompressed tar bytes, writing each regular-file entry into
/// the CAFS and returning the `{in-tarball path → CAFS path}` map plus
/// the per-tarball [`PackageFilesIndex`] row to hand off to the shared
/// store-index writer.
///
/// Non-regular-file entries (symlinks, hardlinks, character / block
/// devices, fifos, GNU / PAX extension headers, directories) are
/// filtered out. Real npm-publish tarballs only carry regular files;
/// anything else would need custom handling that pacquet doesn't yet
/// do, and silently reading a symlink's 0-byte body into the CAFS as
/// if it were a file would just corrupt the store.
///
/// The archive is already fully buffered in memory by the download
/// pipeline. Use `entries_with_seek` + `raw_file_position` to borrow
/// each file payload as a slice of that buffer instead of allocating a
/// fresh `Vec<u8>` and `read_to_end`-ing every entry.
///
/// Every tar-side failure — a corrupt entries iterator, a mangled
/// header (bad mode, bad size), an invalid file offset, a path decode
/// error, a path whose components would escape the CAFS root — comes
/// back as [`TarballError::ReadTarballEntries`] instead of panicking.
/// Non-UTF-8 entry paths are coerced via
/// [`std::path::Path::to_string_lossy`], matching pnpm's string-based
/// handling so a mixed install against the shared `index.db` stays
/// consistent; real-world npm tarballs are UTF-8 so the coercion is
/// almost never hit in practice.
fn extract_tarball_entries(
    tar_data: &[u8],
    store_dir: &StoreDir,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
) -> Result<(HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
    let mut archive = Archive::new(Cursor::new(tar_data));
    let entries = archive
        .entries_with_seek()
        .map_err(TarballError::ReadTarballEntries)?
        // Keep only regular-file `Ok` entries; anything else in the
        // `Ok` arm (directories, symlinks, hardlinks, pax/gnu
        // extension headers, ...) is dropped. `Err` entries fall
        // through so the `?` inside the loop below propagates them —
        // previously this branch did `entry.as_ref().unwrap()` which
        // panicked on any iterator-level error.
        .filter(|entry| match entry {
            Ok(entry) => entry.header().entry_type().is_file(),
            Err(_) => true,
        });

    let ((_, Some(capacity)) | (capacity, None)) = entries.size_hint();

    // Phase 1 (serial): walk the seekable tar stream, validate and clean
    // each regular-file path, and capture the byte slice of its payload.
    // Header parsing has to run sequentially against the single archive
    // stream, but it's cheap; the expensive per-file hashing + CAS write
    // is deferred to the parallel phase below. The bundled `package.json`
    // manifest is captured here too, off the raw payload slice.
    let mut pending: Vec<PendingFile<'_>> = Vec::with_capacity(capacity);
    let mut manifest = None;

    for entry in entries {
        let entry = entry.map_err(TarballError::ReadTarballEntries)?;

        let file_mode = entry.header().mode().map_err(TarballError::ReadTarballEntries)?;
        let file_is_executable = file_mode::is_executable(file_mode);
        let file_size = entry.header().size().map_err(TarballError::ReadTarballEntries)?;
        let data_offset = usize::try_from(entry.raw_file_position()).map_err(|_| {
            TarballError::ReadTarballEntries(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "tar entry file offset does not fit in usize",
            ))
        })?;
        let size = usize::try_from(file_size).map_err(|_| {
            TarballError::ReadTarballEntries(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "tar entry file size does not fit in usize",
            ))
        })?;
        let end = data_offset.checked_add(size).ok_or_else(|| {
            TarballError::ReadTarballEntries(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "tar entry file offset plus size overflows usize",
            ))
        })?;
        let entry_data = tar_data.get(data_offset..end).ok_or_else(|| {
            TarballError::ReadTarballEntries(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "tar entry payload extends beyond archive",
            ))
        })?;

        let entry_path = entry.path().map_err(TarballError::ReadTarballEntries)?;
        // `components().skip(1)` drops the top-level package
        // directory (`package/`). Every remaining component must be
        // `Component::Normal`: a hostile tarball can carry `..`,
        // absolute-root, or Windows-prefix components that — joined
        // onto the CAFS extraction root later in `create_cas_files`
        // — would land files outside the store (directory traversal).
        // Reject loudly rather than silently normalize so tampering
        // is visible.
        //
        // Collect components into a `Vec<String>` and join with `/`
        // rather than going through [`PathBuf::push`] + `to_string_lossy`.
        // `PathBuf` uses the platform's native separator, so on
        // Windows the joined form would be `bin\tool` — which
        // diverges from pnpm's string-based path layer (always `/`)
        // and breaks any [`ignore_file_pattern`] regex / hand-coded
        // matcher that expects forward slashes. The shared `index.db`
        // also has to stay byte-identical to what pnpm writes, so a
        // pacquet install on Windows must emit the same keys.
        // `to_string_lossy()` coerces non-UTF-8 bytes to U+FFFD
        // per-component.
        let mut parts: Vec<String> = Vec::new();
        for component in entry_path.components().skip(1) {
            let Component::Normal(part) = component else {
                return Err(TarballError::ReadTarballEntries(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "tar entry path rejected (non-normal component, possible directory traversal): {entry_path:?}",
                    ),
                )));
            };
            parts.push(part.to_string_lossy().into_owned());
        }
        if parts.is_empty() {
            return Err(TarballError::ReadTarballEntries(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "tar entry path has no payload after dropping the top-level component: {entry_path:?}",
                ),
            )));
        }
        let cleaned_entry_path = parts.join("/");
        // Drop ignored entries before the CAS write. Mirrors
        // upstream's `ignoreFilePattern` semantics: paths are matched
        // *after* the top-level prefix strip, so the callback sees
        // the same strings pnpm's regex does. Bypassing the CAS
        // write here also keeps the package's
        // [`PackageFilesIndex`] tight — an ignored entry never
        // surfaces in `files` or `manifest`.
        if let Some(filter) = ignore_file_pattern
            && filter(&cleaned_entry_path)
        {
            continue;
        }
        // Capture the parsed manifest whenever we see `package.json`.
        // Mirrors pnpm's `bundledManifest` pass-through at
        // [pnpm/pnpm@4750fd370c]: pnpm stuffs the narrowed manifest
        // into `pkgFilesIndex.manifest` so install-side consumers
        // (notably `linkBinsOfDependencies`) can avoid re-reading
        // the file from disk. The [`normalize_bundled_manifest`]
        // pick drops fields downstream code doesn't use, keeping
        // `index.db` rows tight.
        //
        // **Last-entry wins.** Pnpm's [`addFilesFromTarball`] always
        // overwrites `manifestBuffer = fileBuffer` per `package.json`
        // entry (no `if (manifestBuffer === undefined)` guard), so
        // when a tarball contains duplicate `package.json` entries
        // the final one is canonical — same shape as
        // `filesIndex.set(...)` which already overwrites duplicates.
        // Real npm tarballs never publish multiple `package.json`
        // entries, but the consistency with the `files` map is what
        // matters: `manifest` and `files` must describe the same
        // file. Failed JSON parses degrade the field to `None` (the
        // manifest is best-effort; a corrupt `package.json` is the
        // publisher's fault and downstream code can fall back to
        // disk reads).
        //
        // [pnpm/pnpm@4750fd370c]: <https://github.com/pnpm/pnpm/blob/4750fd370c/worker/src/start.ts#L218>
        // [`addFilesFromTarball`]: <https://github.com/pnpm/pnpm/blob/4750fd370c/store/cafs/src/addFilesFromTarball.ts#L41-L43>
        if cleaned_entry_path == "package.json" {
            match serde_json::from_slice::<serde_json::Value>(entry_data) {
                Ok(parsed) => manifest = normalize_bundled_manifest(&parsed),
                Err(error) => {
                    tracing::debug!(
                        ?error,
                        "package.json in tarball failed to parse as JSON; bundled manifest cleared",
                    );
                    manifest = None;
                }
            }
        }

        pending.push(PendingFile {
            cleaned_path: cleaned_entry_path,
            data: entry_data,
            executable: file_is_executable,
            mode: file_mode,
            size: file_size,
        });
    }

    // Phase 2: hash and write every file into the content-addressed
    // store. A tarball used to extract on a single blocking thread, so a
    // package with thousands of files (e.g. `core-js`) pinned one core
    // while the rest sat idle — most costly at the makespan tail, when
    // it's the last extraction still running. `write_cas_entry` is safe
    // to run concurrently, so large tarballs fan out across the dedicated
    // [`cas_write_pool`]; small ones stay serial to skip rayon's per-job
    // dispatch cost when there's nothing to gain. The dedicated pool
    // keeps this off the global pool the linker uses, so an extraction
    // burst can't stall node_modules linking running concurrently.
    const PARALLEL_EXTRACT_THRESHOLD: usize = 32;
    let written: Vec<(String, PathBuf, CafsFileInfo)> =
        if pending.len() >= PARALLEL_EXTRACT_THRESHOLD {
            let write_all = || -> Result<Vec<(String, PathBuf, CafsFileInfo)>, TarballError> {
                pending.par_iter().map(|file| write_cas_entry(store_dir, file)).collect()
            };
            match cas_write_pool() {
                Some(pool) => pool.install(write_all),
                None => write_all(),
            }?
        } else {
            pending.iter().map(|file| write_cas_entry(store_dir, file)).collect::<Result<_, _>>()?
        };

    // Phase 3 (serial): assemble the output maps. `written` preserves
    // `pending` order, so a tarball with duplicate paths keeps the last
    // entry — matching the previous insert-in-order behavior and pnpm's
    // last-wins `filesIndex.set`.
    let mut cas_paths = HashMap::<String, PathBuf>::with_capacity(written.len());
    let mut files = HashMap::with_capacity(written.len());
    for (path, file_path, info) in written {
        if let Some(previous) = cas_paths.insert(path.clone(), file_path) {
            tracing::warn!(?previous, "Duplication detected. Old entry has been ejected");
        }
        if let Some(previous) = files.insert(path, info) {
            tracing::warn!(?previous, "Duplication detected. Old entry has been ejected");
        }
    }

    let pkg_files_idx = PackageFilesIndex {
        manifest,
        requires_build: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
    };
    Ok((cas_paths, pkg_files_idx))
}

/// Walk a zip archive, writing each regular-file entry into the CAFS
/// and returning the `{relative-path → CAFS path}` map plus the
/// per-package [`PackageFilesIndex`] row to hand off to the shared
/// store-index writer. Mirrors the contract of [`extract_tarball_entries`]
/// — same outputs, same per-file CAS write — but for binary
/// `BinaryResolution { archive: zip, prefix: ... }` artifacts (the
/// shape Node.js / Bun / Deno ships their Windows builds in).
///
/// Ports the inner loop of upstream's `extractZipToTarget` at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/fetching/binary-fetcher/src/index.ts>:
///
/// 1. Directory entries are skipped — `AdmZip`'s
///    `extractEntryTo(dir, ...)` expands a directory entry to every
///    descendant via `getEntryChildren`, which would bypass the
///    `ignoreEntry` filter on per-file paths. Iterating only over
///    file entries achieves the same filter coverage.
/// 2. Each entry's path is validated against absolute / `..`
///    components via [`zip::read::ZipFile::enclosed_name`]. Any
///    rejection is surfaced as [`TarballError::PathTraversal`] —
///    same `PATH_TRAVERSAL` error code pnpm raises.
/// 3. If `archive_prefix` is set and the entry path starts with
///    `{prefix}/`, the prefix is stripped before the ignore-filter
///    check and before the entry is recorded in `cas_paths`.
///    Mirrors upstream's `basenamePrefix` slice — the regex sees
///    paths relative to the archive's top-level directory.
/// 4. The cleaned path then runs through `ignore_file_pattern`;
///    matching entries are dropped before any CAS write.
/// 5. The remaining entry's bytes are read and committed via
///    [`StoreDir::write_cas_file`], mirroring upstream's
///    `addFilesFromDir` import step (pnpm extracts to a temp dir then
///    imports; pacquet writes directly to the CAS).
///
/// Unix mode is read off the central-directory record via
/// [`zip::read::ZipFile::unix_mode`]; archives written by Windows
/// tooling don't populate it and we fall back to `0o644`, matching
/// the implicit mode `addFilesFromDir` ends up with after
/// `fs.writeFile` on the temp dir.
fn extract_zip_entries(
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
        algo: "sha512".to_string(),
        files: HashMap::with_capacity(entry_count),
        side_effects: None,
    };

    // Build the `{prefix}/` slice once. Treat `Some("")` as `None`
    // — upstream's `basename === ''` branch keeps entry paths
    // verbatim. The trailing slash anchors the strip so a prefix of
    // `foo` doesn't accidentally consume `foobar/...`.
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
        // single check covers both forms of traversal upstream's
        // `validatePathSecurity` rejects. The returned `PathBuf` has
        // every `.` segment collapsed and is what we use below to
        // build the canonical `cas_paths` / `pkg_files_idx` keys.
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

        // Rebuild the path into a forward-slash string from the
        // sanitized `enclosed_name()` components. Three reasons over
        // using the raw `entry.name()`:
        //
        // 1. `.` segments are already collapsed by `enclosed_name`,
        //    so `pkg/./foo.txt` and `pkg/foo.txt` produce the same
        //    `cas_paths` key — no accidental duplicates from
        //    publisher tooling quirks.
        // 2. The ignore filter sees the same canonical strings the
        //    map is keyed by, so the regex / hand-coded matchers
        //    can't be tripped up by `.` segments either.
        // 3. Zip entries are spec'd to use `/` separators; this also
        //    rejects any `\` an in-the-wild Windows-built archive
        //    might have smuggled in (those would be `Normal`
        //    components on Unix but interpreted as separators on
        //    Windows).
        //
        // `enclosed_name` only yields `Normal` components, so the
        // path-component walk below covers every case.
        let normalized: String = enclosed
            .components()
            .map(|component| match component {
                Component::Normal(name) => name.to_string_lossy().into_owned(),
                _ => unreachable!("enclosed_name returns only Normal components: {:?}", enclosed),
            })
            .collect::<Vec<_>>()
            .join("/");

        // Strip the archive's top-level basename (`prefix` on
        // `pacquet_lockfile::BinaryResolution`) so the ignore filter
        // sees the same relative paths upstream's regex does. If the
        // entry path doesn't start with `{prefix}/` we use the
        // normalized form — pnpm's slice does the same (no-op when
        // the entry already lives at the archive root).
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

        // Same allocation-safety shape as
        // [`extract_tarball_entries`]: clamp the pre-allocation
        // hint at 64 MiB so a maliciously huge `uncompressed_size`
        // in the central directory can't crash the process before
        // `read_to_end` has a chance to surface the real error.
        const MAX_ENTRY_PREALLOC_BYTES: u64 = 64 * 1024 * 1024;
        let prealloc_hint = entry.size().min(MAX_ENTRY_PREALLOC_BYTES) as usize;
        let mut buffer = Vec::new();
        buffer.try_reserve(prealloc_hint).map_err(|err| TarballError::ReadZipEntries {
            url: package_url.to_string(),
            entry_path: cleaned.clone(),
            source: std::io::Error::new(
                std::io::ErrorKind::OutOfMemory,
                format!("failed to reserve {prealloc_hint} bytes for zip entry: {err}"),
            ),
        })?;
        entry.read_to_end(&mut buffer).map_err(|source| TarballError::ReadZipEntries {
            url: package_url.to_string(),
            entry_path: cleaned.clone(),
            source,
        })?;

        // Central-directory record carries a Unix mode only when
        // the archive was built by a Unix tool; Windows-built
        // archives omit it. Fall back to `0o644` so the executable
        // bit defaults to off — `addFilesFromDir` on pnpm's side
        // lands at the same mode after `fs.writeFile`. Mask off
        // the high `st_mode` bits (e.g. `0o100000` for a regular
        // file) so `CafsFileInfo.mode` stays permission-only,
        // matching the convention `add_files_from_dir.rs` enforces
        // for tar / on-disk imports.
        let file_mode = entry.unix_mode().unwrap_or(0o644) & 0o777;
        let file_is_executable = file_mode::is_executable(file_mode);

        let (file_path, file_hash) = store_dir
            .write_cas_file(&buffer, file_is_executable)
            .map_err(TarballError::WriteCasFile)?;

        let file_size = u64::try_from(buffer.len()).unwrap_or(u64::MAX);
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

/// Try to reconstruct the `{filename → CAFS path}` map for a package from
/// the `SQLite` store index, without going to the network. Returns `None`
/// if anything looks off — no index handed in, no row, unreadable row,
/// failed integrity check — so the caller falls through to a fresh
/// download.
///
/// The `verify_store_integrity` parameter matches pnpm's flag of the
/// same name. When `true` (pnpm's default) each referenced CAFS file is
/// stat'ed and compared against the stored `checkedAt`/size, with a
/// re-hash only when the mtime has advanced. When `false` the lookup
/// builds the filename→path map straight from the index row without any
/// filesystem work — missing / corrupt CAFS blobs surface lazily when
/// the caller tries to import them.
///
/// The previous pacquet implementation unconditionally ran a
/// `symlink_metadata` per referenced file and rejected any non-regular
/// dirent outright. That cost a stat syscall per file on every warm
/// install ([#260]) and still diverged from pnpm: the upstream
/// [`checkPkgFilesIntegrity`][1] catches corruption via the content hash
/// and doesn't gate on dirent type.
///
/// [1]: https://github.com/pnpm/pnpm/blob/1819226b51/store/cafs/src/checkPkgFilesIntegrity.ts
///
/// Pre-fetched cas-paths map shared across all per-snapshot futures.
/// Built once at install start by [`prefetch_cas_paths`]; downloads
/// consult it before falling through to a per-snapshot `SQLite` lookup.
///
/// Values are `Arc`-wrapped so the cold-batch fallback can hand a hit
/// back as a cheap pointer-clone rather than memcpy-ing the whole
/// per-file map (each entry is a `HashMap<String, PathBuf>` with up
/// to ~hundred entries, and Copilot reasonably flagged the deep clone
/// as a hot-path cost).
///
/// [#260]: https://github.com/pnpm/pacquet/issues/260
pub type PrefetchedCasPaths = HashMap<String, Arc<HashMap<String, PathBuf>>>;

/// Bundled package manifests recovered from the `SQLite` store index,
/// keyed by the same `<integrity>\t<pkg_id>` string [`PrefetchedCasPaths`]
/// uses. Mirrors pnpm's `bundledManifest` cache in
/// [`worker/src/start.ts`](https://github.com/pnpm/pnpm/blob/4750fd370c/worker/src/start.ts#L144):
/// pnpm reads the parsed manifest out of `pkgFilesIndex.manifest` so
/// `linkBinsOfDependencies` doesn't have to re-read `package.json`
/// from disk per child. Each value is `Arc`-wrapped so multiple
/// bin-link consumers can hold the same parsed manifest without
/// deep-cloning.
///
/// Only keys whose row carried a manifest blob appear in the map —
/// a missing key means either "row exists but has no manifest" (old
/// pacquet write, or a tarball whose `package.json` failed to
/// parse) or "package wasn't prefetched at all". Callers that need
/// to tell those apart cross-reference with [`PrefetchedCasPaths`]
/// from the same [`PrefetchResult`].
pub type PrefetchedManifests = HashMap<String, Arc<serde_json::Value>>;

/// Side-effects-cache overlays recovered from the same `index.db`
/// rows as [`PrefetchedCasPaths`]. The outer key is the same
/// `<integrity>\t<pkg_id>` store-index row key; the inner map is
/// the per-row `cache_key → FilesMap` table that `VerifyResult`
/// produces (already with the `added` / `deleted` overlay applied
/// against the base files). Mirrors the shape pnpm threads through
/// `PackageFilesResponse.sideEffectsMaps` at
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/store/create-cafs-store/src/index.ts#L83-L100>.
///
/// Pacquet hands these off to `BuildModules`'s `is_built` gate —
/// the build-phase skips a snapshot when its computed
/// `calc_dep_state` cache key has a matching entry here.
///
/// Outer values are `Arc`-wrapped for the same cold-batch cheap-clone
/// reason `PrefetchedCasPaths` is.
pub type PrefetchedSideEffectsMaps =
    HashMap<String, Arc<HashMap<String, HashMap<String, PathBuf>>>>;

/// Output of [`prefetch_cas_paths`]: the warm-cache filesystem map
/// plus any bundled manifests and side-effects overlays recovered
/// from the same `index.db` rows. Bundled in a single struct so
/// callers can destructure all three after one `await`, rather than
/// the function having to thread three separate `spawn_blocking`
/// round-trips through.
#[derive(Default)]
pub struct PrefetchResult {
    pub cas_paths: PrefetchedCasPaths,
    pub manifests: PrefetchedManifests,
    pub side_effects_maps: PrefetchedSideEffectsMaps,
}

/// Batch the entire warm-cache lookup phase into one `spawn_blocking`
/// task at install start: collect every row the lockfile is going to
/// ask about under a single `index.lock()` round-trip, drop the lock,
/// then run the per-package integrity checks unlocked. Returns a
/// `cache_key → Arc<cas_paths>` map the per-snapshot futures can hit
/// synchronously.
///
/// **Locking shape (per Copilot review on [#292]):** the `SQLite` mutex
/// is held only for the SELECT loop. Integrity checks (`fs::metadata`
/// per file, optional re-hash) happen after the guard drops, so a
/// concurrent reader on the same `SharedReadonlyStoreIndex` doesn't
/// have to wait through the whole batch's filesystem work.
///
/// **Why one batched task instead of 1352 `spawn_blockings`:** the
/// per-snapshot path fans out one `tokio::task::spawn_blocking` per
/// snapshot. With 1352 snapshots all firing into the default
/// 512-thread blocking pool, threads compete for CPU and get
/// preempted between fs ops — sample-profiling showed cache-lookup
/// bodies averaging 20-60 ms each (sum 26-82 s) almost entirely
/// blocked, even though the actual SELECT (≈40 µs) and per-file
/// integrity stats (≈ms each) shouldn't take that long. Doing the
/// whole batch on one thread avoids the OS-scheduler / kernel-journal
/// thrash and makes each query fast in CPU-time. Pnpm's piscina pool
/// achieves the same shape implicitly with 4 dedicated workers.
///
/// Cache misses (no row, malformed row, integrity-check failure)
/// just don't appear in the result. The caller then falls through
/// to [`DownloadTarballToStore::run_without_mem_cache`] for those
/// keys, which still has its own cache check as a backstop.
///
/// [#292]: https://github.com/pnpm/pacquet/pull/292
pub async fn prefetch_cas_paths(
    index: Option<SharedReadonlyStoreIndex>,
    store_dir: &'static StoreDir,
    cache_keys: Vec<String>,
    verify_store_integrity: bool,
    verified_files_cache: SharedVerifiedFilesCache,
) -> PrefetchResult {
    let Some(index) = index else { return PrefetchResult::default() };
    if cache_keys.is_empty() {
        return PrefetchResult::default();
    }
    let result = tokio::task::spawn_blocking(move || -> PrefetchResult {
        // Phase 1: read every row's *raw bytes* under the mutex.
        // Splitting raw-read from decode means the
        // `SharedReadonlyStoreIndex` lock is held only for the
        // SELECT loop, not for the per-row msgpackr decode — which
        // is the dominant CPU cost once rows carry a `manifest`
        // field (transcode + `rmp_serde::from_slice` of a nested
        // JSON tree per row, times ~1k rows on a real lockfile).
        // Doing the decode after the guard drops lets it fan out
        // across rayon below.
        //
        // One batched `SELECT ... WHERE key IN (?, ?, ...)` per
        // `GET_MANY_CHUNK` (see `StoreIndex::get_many_raw`)
        // collapses what used to be N round-trips into one — see
        // <https://github.com/pnpm/pacquet/issues/294> for the cold-cache regression the per-key loop
        // introduced when every key missed.
        let raw: Vec<(String, Vec<u8>)> = {
            let Ok(guard) = index.lock() else {
                tracing::debug!(
                    target: "pacquet::download",
                    "store-index mutex poisoned at prefetch start; falling back to per-snapshot lookups",
                );
                return PrefetchResult::default();
            };
            match guard.get_many_raw(&cache_keys) {
                Ok(rows) => rows,
                Err(error) => {
                    tracing::debug!(
                        target: "pacquet::download",
                        ?error,
                        "store-index batched read failed at prefetch start; falling back to per-snapshot lookups",
                    );
                    return PrefetchResult::default();
                }
            }
        };
        // Phase 2: decode each row's msgpackr-records bytes into a
        // `PackageFilesIndex`, then run the integrity check. Both
        // steps are per-row CPU work with no shared state, so we
        // fan out across rayon. With manifests included in the
        // payload, decoding 1k+ rows serially had become the
        // dominant chunk of the prefetch wall (single-threaded
        // `spawn_blocking`); the par-iter recovers the per-row
        // parallelism the warm-batch link phase already uses.
        //
        // The bundled manifest is split off the decoded entry via
        // `Option::take` so it travels back to the caller without
        // an intermediate `Value::clone` of the JSON tree — the
        // verify function only inspects `files`, never `manifest`.
        let decoded: Vec<(String, Option<Arc<serde_json::Value>>, pacquet_store_dir::VerifyResult)> = raw
            .into_par_iter()
            .filter_map(|(cache_key, bytes)| {
                let mut entry: PackageFilesIndex = match pacquet_store_dir::decode_package_files_index(&bytes) {
                    Ok(entry) => entry,
                    Err(error) => {
                        tracing::debug!(
                            target: "pacquet::download",
                            ?cache_key,
                            ?error,
                            "skipping undecodable package_index row at prefetch",
                        );
                        return None;
                    }
                };
                let manifest = entry.manifest.take().map(Arc::new);
                let verify_result = if verify_store_integrity {
                    pacquet_store_dir::check_pkg_files_integrity(
                        store_dir,
                        entry,
                        &verified_files_cache,
                    )
                } else {
                    pacquet_store_dir::build_file_maps_from_index(store_dir, entry)
                };
                Some((cache_key, manifest, verify_result))
            })
            .collect();

        let mut cas_paths = HashMap::with_capacity(decoded.len());
        let mut manifests = HashMap::new();
        let mut side_effects_maps = HashMap::new();
        for (cache_key, manifest, verify_result) in decoded {
            if verify_result.passed {
                if let Some(manifest) = manifest {
                    manifests.insert(cache_key.clone(), manifest);
                }
                if let Some(maps) = verify_result.side_effects_maps
                    && !maps.is_empty()
                {
                    side_effects_maps.insert(cache_key.clone(), Arc::new(maps));
                }
                cas_paths.insert(cache_key, Arc::new(verify_result.files_map));
            }
        }
        PrefetchResult { cas_paths, manifests, side_effects_maps }
    })
    .await;
    result.unwrap_or_else(|error| {
        tracing::warn!(
            target: "pacquet::download",
            ?error,
            "store-index prefetch task failed; falling back to per-snapshot lookups",
        );
        PrefetchResult::default()
    })
}

/// The `index` argument is a shared read-only handle that callers open
/// once per install and pass in repeatedly, so we don't pay the
/// `Connection::open` + PRAGMA cost per package.
async fn load_cached_cas_paths(
    index: Option<SharedReadonlyStoreIndex>,
    store_dir: &'static StoreDir,
    cache_key: String,
    verify_store_integrity: bool,
    verified_files_cache: SharedVerifiedFilesCache,
) -> Option<HashMap<String, PathBuf>> {
    let index = index?;
    // Hold on to a copy of the cache key for the outer `JoinError` log,
    // since the task body moves the original in.
    let outer_cache_key = cache_key.clone();
    let result = tokio::task::spawn_blocking(move || -> Option<HashMap<String, PathBuf>> {
        // Treat a poisoned mutex as a cache miss rather than propagating the
        // panic: the `SELECT` is stateless, so the prior panic couldn't have
        // left the index in an inconsistent shape, and cache lookups are a
        // best-effort hint anyway — failing over to a fresh download is the
        // more resilient default than turning every subsequent snapshot into
        // a crash.
        let entry = {
            let Ok(guard) = index.lock() else {
                tracing::debug!(
                    target: "pacquet::download",
                    ?cache_key,
                    "store-index mutex poisoned; treating cache lookup as a miss",
                );
                return None;
            };
            guard.get(&cache_key).ok()?
        }?;

        let verify_result = if verify_store_integrity {
            pacquet_store_dir::check_pkg_files_integrity(store_dir, entry, &verified_files_cache)
        } else {
            pacquet_store_dir::build_file_maps_from_index(store_dir, entry)
        };
        if !verify_result.passed {
            // Per-file reason (filename, CAS path, size mismatch, hash
            // mismatch, ...) is logged at `debug!` inside
            // `check_pkg_files_integrity` / `build_file_maps_from_index`
            // where the failure actually happens — this caller-side log
            // just summarises "the row as a whole didn't verify" so log
            // scrapers can correlate the per-file debug lines with the
            // snapshot they belong to.
            tracing::debug!(
                target: "pacquet::download",
                ?cache_key,
                "store-index entry failed integrity check; re-fetching",
            );
            return None;
        }
        Some(verify_result.files_map)
    })
    .await;

    match result {
        Ok(cas_paths) => cas_paths,
        Err(error) => {
            // `JoinError` — the blocking task panicked, or the runtime was
            // cancelled mid-install. Degrade to a cache miss so the caller
            // falls through to a fresh download, but surface the error so
            // the panic / cancellation stays diagnosable.
            tracing::warn!(
                target: "pacquet::download",
                ?error,
                cache_key = ?outer_cache_key,
                "store-index lookup task failed; treating cache lookup as a miss",
            );
            None
        }
    }
}

/// This subroutine downloads and extracts a tarball to the store directory.
///
/// It returns a CAS map of files in the tarball.
///
/// `Clone` is cheap — every field is a reference, a `Copy` scalar, or an
/// `Arc` — so a caller can keep a copy to retry through a different entry
/// point (e.g. fall back to [`Self::run_without_mem_cache`] after a
/// best-effort [`Self::run_with_mem_cache`] reports a sibling failure).
#[derive(Clone)]
#[must_use]
pub struct DownloadTarballToStore<'a> {
    pub http_client: &'a ThrottledClient,
    pub store_dir: &'static StoreDir,
    /// Shared read-only handle to the `SQLite` store index. `None` when the
    /// store does not (yet) have an `index.db`, in which case every cache
    /// lookup short-circuits to a network fetch. Callers open this once per
    /// install and pass the same handle to every `DownloadTarballToStore`
    /// so we don't reopen the DB per package.
    pub store_index: Option<SharedReadonlyStoreIndex>,
    /// Handle to the batched store-index writer. Each successful tarball
    /// extraction queues one `(key, PackageFilesIndex)` row; a single
    /// writer task drains the channel and flushes batches of up to 256 in
    /// one transaction each, so the whole install goes through one
    /// `Connection::open` and a handful of WAL commits instead of the old
    /// "open + PRAGMA + insert + drop" per tarball (which ballooned
    /// tokio's blocking pool to 500+ threads on a 1352-snapshot install —
    /// see [#263]). `None` degrades to "skip index row", matching the read
    /// side's stance: install still succeeds, the next install misses on
    /// this cache key and re-downloads.
    ///
    /// [#263]: https://github.com/pnpm/pacquet/issues/263
    pub store_index_writer: Option<Arc<StoreIndexWriter>>,
    /// Mirrors pnpm's `verify-store-integrity` / `verifyStoreIntegrity`
    /// setting. When `true` (pnpm's default) each cached CAFS file is
    /// stat'ed and optionally re-hashed before reuse. When `false` the
    /// index is trusted and the import fails lazily if a blob is
    /// missing — trades the per-file stat / optional rehash for the
    /// risk that a mutated or corrupt store serves stale content until
    /// the next integrity-full install. Whether that translates into a
    /// wall-time win depends on the workload; the per-snapshot stat
    /// isn't the bottleneck on the benchmarks this repo tracks (see
    /// [#273]), but cutting the syscall count is still correct.
    ///
    /// [#273]: https://github.com/pnpm/pacquet/issues/273
    pub verify_store_integrity: bool,
    /// Install-scoped dedup cache shared across every cached-tarball
    /// lookup. Ports pnpm's `verifiedFilesCache: Set<string>`: a CAFS
    /// path that one snapshot's verify pass has already stat'ed (and
    /// optionally re-hashed) gets skipped when the next snapshot
    /// touches the same blob. Without it pacquet was paying the
    /// per-file stat in `check_pkg_files_integrity` once per
    /// (snapshot × file) instead of once per (file). Allocate one
    /// `Arc<DashSet<PathBuf>>` at install bootstrap and pass the same
    /// handle to every `DownloadTarballToStore`.
    pub verified_files_cache: SharedVerifiedFilesCache,
    pub package_integrity: &'a Integrity,
    pub package_unpacked_size: Option<usize>,
    /// `dist.fileCount` when the registry published one. Combined with
    /// `package_unpacked_size` into the download's queueing priority —
    /// per-file pipeline overhead (CAS write syscalls, hashing) makes a
    /// many-small-files package as slow to finish as a much larger
    /// few-files one.
    pub package_file_count: Option<usize>,
    pub package_url: &'a str,
    /// Stable identifier for the package, e.g. `"{name}@{version}"`. Paired
    /// with `package_integrity` to form the `SQLite` index key per pnpm v11's
    /// `storeIndexKey`.
    pub package_id: &'a str,
    /// URL-keyed `Authorization` header lookup, built from the parsed
    /// `.npmrc` creds. Resolved per request so a tarball served from a
    /// different host than the registry still picks up its own header.
    /// Mirrors pnpm's
    /// [`getAuthHeaderByURI`](https://github.com/pnpm/pnpm/blob/601317e7a3/network/auth-header/src/index.ts)
    /// pattern.
    pub auth_headers: &'a AuthHeaders,
    /// Install root the fetch belongs to. Threaded into the
    /// `pnpm:progress` `requester` field on `fetched` /
    /// `found_in_store` events. Same value as the
    /// [`pacquet_reporter::StageLog::prefix`] computed in
    /// `Install::run`.
    pub requester: &'a str,
    /// Pre-fetched cache lookups built once at install start
    /// ([`prefetch_cas_paths`]). When `Some`, this is consulted first;
    /// the per-snapshot `SQLite` + integrity-check round-trip is skipped
    /// for every key already resolved by the prefetch.
    pub prefetched_cas_paths: Option<&'a PrefetchedCasPaths>,
    /// Per-attempt retry budget for the tarball pipeline. Mirrors pnpm's
    /// `fetch-retries*` knobs (`network/fetch/src/fetch.ts`,
    /// `fetching/tarball-fetcher/src/remoteTarballFetcher.ts`): every
    /// failure retries except HTTP 401, 403, 404 — including arbitrary
    /// 4xx / 5xx, network resets, timeouts, mid-stream body errors,
    /// integrity mismatches, and gzip / tar parse failures ([#259]).
    ///
    /// [#259]: https://github.com/pnpm/pacquet/issues/259
    pub retry_opts: RetryOpts,
    /// Per-package archive-entry filter applied during CAS extraction.
    /// Receives the entry's path *after* the top-level
    /// `package/` strip; returning `true` drops the entry before the
    /// CAS write. Mirrors upstream's `ignoreFilePattern` /
    /// `archiveFilters` regex at
    /// <https://github.com/pnpm/pnpm/blob/94240bc046/fetching/binary-fetcher/src/index.ts>.
    /// `None` (the default for ordinary npm tarballs) writes every
    /// regular-file entry; `Some(filter)` is what the binary fetcher
    /// uses to strip Node's bundled `npm` / `corepack` from the CAS.
    ///
    /// Stored as `Arc` so the install dispatcher (Slice D) can
    /// construct one filter per fetch from runtime config — e.g.
    /// `archiveFilters` keyed by `pkg.name` — without leaking
    /// memory or pinning the filter to `'static`. Cloning the
    /// Arc per retry attempt is cheap; the inner trait object
    /// is shared.
    pub ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
    /// `offline` from `Config`. When `true` and both the warm
    /// prefetch (`prefetched_cas_paths`) and the `SQLite` `index.db`
    /// lookup (`load_cached_cas_paths`) miss, the fetcher fails fast
    /// with [`TarballError::NoOfflineTarball`] rather than hitting
    /// the registry. The upstream `--offline` flag gates the
    /// metadata-fetch path inside `pickPackage`; pacquet has no
    /// metadata-fetch path on the frozen-install flow (the lockfile
    /// pins every resolution), so this gate is pacquet's most useful
    /// interpretation of the flag for frozen installs.
    pub offline: bool,
    /// Install-scoped set used to de-duplicate package-status progress.
    /// When `Some`, a `fetched` or `found_in_store` emit records its
    /// `store_index_key(integrity, pkg_id)` here. Later callers that see
    /// the same key skip their own package-status emit, while still doing
    /// the underlying fetch/cache work. Only the fresh install path
    /// threads this set through, because resolve-time prefetches can
    /// otherwise report the same package again in the warm batch.
    pub progress_reported: Option<SharedReportedProgressKeys>,
}

/// Project [`TarballError`] onto pnpm's `requestRetryLogger`'s
/// JS-shaped error object. The JS default-reporter dispatches on
/// `httpStatusCode ?? status ?? errno ?? code` to render the retry
/// reason; absent fields skip rather than emit `null` so the `??`
/// chain doesn't short-circuit on a present-but-`null` field.
///
/// Today pacquet populates `http_status_code` for the
/// [`TarballError::HttpStatus`] variant and a curated
/// `ERR_PACQUET_*` constant in `code` for every other variant —
/// the mapping is hand-maintained per match arm rather than
/// reflectively derived, so renaming a [`TarballError`] variant
/// won't silently change the emitted `code`. `errno` and `status`
/// are skipped because pacquet's error layer doesn't carry them;
/// pnpm's emit fills them when the underlying network error did.
fn tarball_error_to_request_retry(err: &TarballError) -> RequestRetryError {
    let mut out = RequestRetryError {
        message: err.to_string(),
        http_status_code: None,
        status: None,
        errno: None,
        code: None,
    };
    match err {
        TarballError::HttpStatus(http) => {
            out.http_status_code = Some(http.status.to_string());
        }
        TarballError::FetchTarball(_) => {
            out.code = Some("ERR_PACQUET_FETCH".to_string());
        }
        TarballError::Checksum(_) => {
            out.code = Some("ERR_PACQUET_TARBALL_INTEGRITY".to_string());
        }
        TarballError::DecodeGzip(_) => {
            out.code = Some("ERR_PACQUET_TARBALL_GZIP".to_string());
        }
        TarballError::ReadTarballEntries(_) => {
            out.code = Some("ERR_PACQUET_TARBALL_TAR".to_string());
        }
        TarballError::WriteCasFile(_) | TarballError::WriteStoreIndex(_) => {
            out.code = Some("ERR_PACQUET_TARBALL_STORE".to_string());
        }
        TarballError::TaskJoin(_) => {
            out.code = Some("ERR_PACQUET_TASK_JOIN".to_string());
        }
        TarballError::TarballTooLarge { .. } => {
            out.code = Some("ERR_PACQUET_TARBALL_TOO_LARGE".to_string());
        }
        TarballError::SiblingFetchFailed { .. } => {
            out.code = Some("ERR_PACQUET_SIBLING_FETCH".to_string());
        }
        TarballError::PathTraversal { .. } => {
            out.code = Some("ERR_PACQUET_PATH_TRAVERSAL".to_string());
        }
        TarballError::ReadZipArchive { .. } | TarballError::ReadZipEntries { .. } => {
            out.code = Some("ERR_PACQUET_ZIP".to_string());
        }
        TarballError::NoOfflineTarball { .. } => {
            // The retry classifier sees this only if the offline gate
            // were ever placed inside the retry loop (it isn't —
            // `NoOfflineTarball` short-circuits before
            // `fetch_and_extract_with_retry`). The arm exists for
            // exhaustiveness; the `code` field mirrors the upstream
            // shape so a future surface that does run this error
            // through the retry logger renders the right code.
            out.code = Some("ERR_PACQUET_NO_OFFLINE_TARBALL".to_string());
        }
    }
    out
}

/// Whether a [`TarballError`] from one tarball-fetch attempt should be
/// retried. Matches pnpm's
/// [`remoteTarballFetcher.ts`](https://github.com/pnpm/pnpm/blob/1819226b51/fetching/tarball-fetcher/src/remoteTarballFetcher.ts#L76-L84)
/// policy *exactly*: only HTTP 401, 403, 404 (and the git-prepare
/// failure code, which doesn't apply to registry tarballs) fail fast.
/// Every other failure — arbitrary 4xx, 5xx, network reset, timeout,
/// integrity mismatch, gzip / tar parse error, CAFS write hiccup —
/// retries until the budget is exhausted.
///
/// In particular this means we retry integrity mismatches and decode
/// errors. pnpm wraps the body fetch *and* the post-download
/// `addFilesFromTarball` (integrity check + extraction) in one retried
/// closure for the same reason: a corrupted byte on the wire that
/// happens to escape TCP framing can break either the integrity check
/// or the gzip decode, and a re-fetch is the cheapest way out.
fn is_transient_error(err: &TarballError) -> bool {
    match err {
        TarballError::HttpStatus(http) => !matches!(http.status, 401 | 403 | 404),
        _ => true,
    }
}

/// Run one full tarball-fetch attempt: hit the network, drain the body
/// into RAM, verify the integrity hash, then decompress and extract
/// every entry into the CAFS. Returns the cas-paths map and the
/// per-tarball [`PackageFilesIndex`] row that the caller queues into
/// the shared store-index writer once the retry loop succeeds.
///
/// The whole pipeline lives in one attempt because pnpm's tarball
/// fetcher does the same: any failure inside `addFilesFromTarball`
/// (integrity mismatch, gzip decode, malformed tar) propagates back
/// to the retry boundary so a re-fetch can recover from a flaky
/// transfer that happens to checksum or decode wrong.
///
/// Permits are acquired *inside* this function so a backoff sleep
/// between attempts doesn't keep one parked. The network permit is
/// held from `connect + send` through body streaming (matching pnpm's
/// pQueue and [#281]'s EMFILE fix), then dropped before the
/// `post_download_semaphore` permit gates the CPU-bound checksum +
/// decode + extract step.
///
/// [#281]: https://github.com/pnpm/pacquet/pull/281
#[expect(
    clippy::too_many_arguments,
    reason = "arg count is set by upstream pnpm's fetcher signature"
)]
async fn fetch_and_extract_once<Reporter: self::Reporter>(
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
) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
    let network_error =
        |error| TarballError::FetchTarball(NetworkError { url: package_url.to_string(), error });

    // Acquire the network permit *before* `connect + send` and hold it
    // through body streaming. Releasing earlier would let the next
    // batch of futures `connect()` while previous bodies are still
    // draining, breaking the bound on concurrent open sockets.
    //
    // `acquire_for_url_with_priority` routes the request through the
    // per-registry TLS-configured client when one is set for
    // `package_url`'s nerf-darted prefix, falling back to the default
    // client otherwise. Tarball hosts that differ from the metadata
    // host still pick up the right per-registry client because the
    // 5-step `pickSettingByUrl` lookup also matches on the tarball
    // URL. When the pool is saturated, the package with the most
    // estimated pipeline work claims the next freed slot, so the
    // longest download+extract jobs never start last.
    let client = http_client.acquire_for_url_with_priority(package_url, download_priority).await;
    let mut request = client.get(package_url);
    // Match pnpm's tarball download path
    // ([`remoteTarballFetcher.ts`](https://github.com/pnpm/pnpm/blob/601317e7a3/fetching/tarball-fetcher/src/remoteTarballFetcher.ts#L66-L70)):
    // resolve the per-URL auth header and attach it. Tarball hosts that
    // differ from the metadata host still pick up the header keyed at
    // the registry's nerf-darted URI.
    if let Some(value) = auth_headers.for_url(package_url) {
        request = request.header("authorization", value);
    }

    // `pnpm:fetching-progress started` mirrors pnpm's per-attempt
    // emit at
    // <https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/package-requester/src/packageRequester.ts#L560>.
    // Fires exactly once per HTTP attempt — including attempts that
    // fail before the response head arrives (DNS / connect /
    // timeout) so retried attempts stay visible in the reporter.
    // `size` is the response's `Content-Length` when we have a
    // response head, and JSON `null` (i.e. `None`) when we don't:
    // either because the response is chunked / unknown-length, or
    // because the request errored out before headers. pnpm's
    // reporter checks `size != null` before rendering a percent
    // gauge, so this admits "we don't know yet" only when we truly
    // don't know.
    //
    // `attempt` is one-indexed (the in-flight attempt) to match
    // pnpm's wire shape — `node-retry`'s `op.attempt(cb)` callback
    // hands `cb` a 1-indexed counter, which `packageRequester`
    // forwards verbatim into the `attempt` field. Pacquet's loop
    // counter is zero-indexed, so emit `attempt + 1`. The default
    // reporter's `reportBigTarballsProgress` filters on
    // `log.attempt === 1` (so retries don't reset the progress
    // line), so a zero would silence every "Downloading ..." line.
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
        // Drain small error bodies so reqwest/hyper can return the
        // connection to the keep-alive pool — dropping an unconsumed
        // `Response` closes the underlying connection, which we'd then
        // pay to reopen on retry. Skip the drain when the body is
        // unknown-length or larger than the cap, since hyper only
        // returns the connection to the pool once the body is fully
        // consumed; a partial drain wouldn't help and would just buffer
        // a pathological response.
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

        // `in_progress` is gated and throttled to match pnpm exactly
        // (see [`remoteTarballFetcher.ts`](https://github.com/pnpm/pnpm/blob/086c5e91e8/fetching/tarball-fetcher/src/remoteTarballFetcher.ts#L143)):
        //
        // 1. Only emit when the tarball size is *known* (i.e. the
        //    response carried a `Content-Length`). Chunked / unknown-
        //    length responses skip the channel entirely; pnpm's
        //    default reporter needs `size` to render a percent gauge,
        //    and emitting `downloaded` without a denominator is
        //    noise.
        // 2. Only emit when the tarball is "big" — at least 5 MB.
        //    Most npm packages are well under this; the pnpm authors
        //    found per-byte progress for tiny tarballs floods the
        //    JS side with values that would render as 100% before any
        //    UI tick can show them.
        // 3. Throttle emits to 500ms with leading + trailing edges,
        //    matching `lodash.throttle(opts.onProgress, 500)`. Leading
        //    is the first chunk we see; trailing is a final emit
        //    after the body finishes so the consumer sees the actual
        //    end-of-download byte count, not whatever value happened
        //    to be cached at the last 500ms tick.
        const BIG_TARBALL_SIZE: u64 = 5 * 1024 * 1024;
        const IN_PROGRESS_THROTTLE: Duration = Duration::from_millis(500);
        let emit_progress = expected_size.is_some_and(|size| size >= BIG_TARBALL_SIZE);
        // `None` means the leading-edge emit hasn't happened yet, so
        // the first chunk always fires. Subsequent chunks fire only
        // when the previous emit was at least 500ms ago.
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
        // Trailing emit: matches `lodash.throttle`'s default
        // `{leading: true, trailing: true}`. Without it the last
        // partial window is dropped — a download that ends 200ms
        // after the previous tick would leave consumers stuck at a
        // stale `downloaded` value below the real total.
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

    // Body fully buffered; release the network permit before the
    // CPU-bound work so spawn_blocking doesn't hold one of the
    // limited fetch slots.
    //
    // The network permit was the only gate during fetch + body
    // buffering — `default_network_concurrency()` bounds concurrent
    // open sockets and concurrent in-progress fetches. The buffer
    // lives in RAM across this drop and the next acquire, so a
    // pathologically slow decompression stage could let buffered
    // tarballs accumulate beyond the network bound. In practice
    // flate2 decompresses faster than the network delivers, so
    // buffered-but-not-yet-decompressing tarballs stay close to zero.
    // Gating body buffering with `post_download_semaphore` (the
    // smaller `num_cpus * 2` cap) instead would pin `network_concurrency`
    // permits waiting for it and collapse fetch concurrency down to
    // `post_download` — that's the regression `perf(tarball)` (a43ca32)
    // fixed; don't reintroduce it.
    drop(client);

    // Gate the CPU-heavy decompress + cafs-write pipeline. The blocking
    // pool is 512-wide by default, which is right for I/O wait but
    // disastrous for CPU work that can only really run `num_cpus` at a
    // time, so we cap concurrent `spawn_blocking` bodies. The permit is
    // held across the `spawn_blocking.await` below and dropped at end
    // of scope.
    let _post_download_permit = post_download_semaphore()
        .acquire()
        .await
        .expect("post-download semaphore shouldn't be closed this soon");

    tracing::info!(target: "pacquet::download", ?package_url, "Download completed");

    // Move the CPU-bound work (SHA-512, gzip inflate, per-file SHA-512,
    // CAFS writes) onto the blocking pool. Same reasoning as before the
    // retry refactor: a plain `tokio::spawn` pinned a reactor worker for
    // each tarball — on a 2-core runner only two tarballs could make
    // progress at a time. The post-download semaphore caps concurrency
    // here.
    let expected_integrity = expected_integrity.cloned();
    let package_url_owned = package_url.to_string();
    let result = tokio::task::spawn_blocking(
        move || -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
            // Verify a known integrity, or compute one when the hash
            // isn't known until after download — remote (non-registry)
            // https-tarball direct deps, where the resolver learns the
            // integrity here. Mirrors pnpm's worker
            // `integrity ?? calcIntegrity(buffer)`
            // ([worker/src/start.ts](https://github.com/pnpm/pnpm/blob/086c5e91e8/worker/src/start.ts#L232)).
            let integrity = if let Some(expected) = expected_integrity {
                expected.check(&buffer).map_err(|error| {
                    TarballError::Checksum(VerifyChecksumError { url: package_url_owned, error })
                })?;
                expected
            } else {
                let mut opts = IntegrityOpts::new().algorithm(Algorithm::Sha512);
                opts.input(&buffer);
                opts.result()
            };

            // Extract in a scope so the decompressed buffer + `tar::Archive`
            // are released before we return — a large package's inflated
            // bytes can be many MB.
            let (cas_paths, pkg_files_idx) = {
                let tar_data = decompress_gzip(&buffer, package_unpacked_size)?;
                extract_tarball_entries(&tar_data, store_dir, ignore_file_pattern.as_deref())?
            };
            Ok((integrity, cas_paths, pkg_files_idx))
        },
    )
    .await
    .map_err(TarballError::TaskJoin)??;

    tracing::info!(target: "pacquet::download", ?package_url, "Checksum verified");

    Ok(result)
}

/// Run [`fetch_and_extract_once`] under pnpm's retry policy. Permanent
/// errors (HTTP 401 / 403 / 404 — see [`is_transient_error`]) fail on
/// the first attempt; everything else sleeps with exponential backoff
/// and tries again until the budget is exhausted, surfacing the most
/// recent error.
///
/// On retry, CAFS writes from a previous attempt that may have made it
/// part-way through extraction stay on disk. That's safe: the CAFS is
/// content-addressed, so re-extracting the same bytes produces
/// identical paths and `write_cas_file` is idempotent.
/// Emit `pnpm:progress found_in_store` for a (`package_id`, requester)
/// pair the cache resolved without a download. Mirrors pnpm's emit at
/// <https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/package-requester/src/packageRequester.ts#L435>.
fn emit_progress_found_in_store<Reporter: self::Reporter>(
    package_id: &str,
    requester: &str,
    progress_key: Option<(&SharedReportedProgressKeys, &str)>,
) {
    if progress_already_reported(progress_key) {
        return;
    }
    Reporter::emit(&LogEvent::Progress(ProgressLog {
        level: LogLevel::Debug,
        message: ProgressMessage::FoundInStore {
            package_id: package_id.to_owned(),
            requester: requester.to_owned(),
        },
    }));
}

fn emit_progress_fetched<Reporter: self::Reporter>(
    package_id: &str,
    requester: &str,
    progress_key: Option<(&SharedReportedProgressKeys, &str)>,
) {
    if progress_already_reported(progress_key) {
        return;
    }
    Reporter::emit(&LogEvent::Progress(ProgressLog {
        level: LogLevel::Debug,
        message: ProgressMessage::Fetched {
            package_id: package_id.to_owned(),
            requester: requester.to_owned(),
        },
    }));
}

fn progress_already_reported(progress_key: Option<(&SharedReportedProgressKeys, &str)>) -> bool {
    progress_key.is_some_and(|(reported, key)| !reported.insert(key.to_owned()))
}

/// Byte-equivalent cost of one file's fixed pipeline overhead (the
/// CAS-write syscalls and hash setup paid per file regardless of its
/// size, ~75 µs against a pipeline that moves a byte through
/// download + decompress + hash + write in ~25 ns). Folding it into
/// the priority makes a many-small-files package rank as the long
/// job it actually is: extraction cost, not just transfer cost,
/// decides when a package's pipeline work finishes.
const PRIORITY_BYTES_PER_FILE: u64 = 3_000;

/// Queueing priority of a tarball download: the package's estimated
/// total pipeline work (transfer + decompress + hash + CAS writes) in
/// byte-equivalents. Missing hints contribute zero, so a package with
/// no published `dist` stats queues behind every estimated one.
#[must_use]
pub fn download_priority(unpacked_size: Option<usize>, file_count: Option<usize>) -> u64 {
    let size = unpacked_size.map_or(0, |size| size as u64);
    let per_file =
        file_count.map_or(0, |count| (count as u64).saturating_mul(PRIORITY_BYTES_PER_FILE));
    // `UNPRIORITIZED` (`u64::MAX`) is the latency-class sentinel; a
    // hostile registry publishing absurd `dist` stats must not be able
    // to saturate a download's priority into that class.
    size.saturating_add(per_file).min(UNPRIORITIZED - 1)
}

// 9 arguments — over the default clippy threshold but each is
// distinct: client + URL + integrity describe the request, ID +
// requester are the reporter dimensions, unpacked-size is allocation
// hinting, store_dir + retry_opts are install-scoped, and
// ignore_file_pattern is the per-fetch archive filter. Bundling
// into a struct would just push the same fields into a wrapper.
#[allow(
    clippy::too_many_arguments,
    reason = "the parameters are independent install-scoped inputs; bundling them into a struct only moves the same fields into a wrapper"
)]
async fn fetch_and_extract_with_retry<Reporter: self::Reporter>(
    http_client: &ThrottledClient,
    package_url: &str,
    expected_integrity: Option<&Integrity>,
    package_unpacked_size: Option<usize>,
    download_priority: u64,
    package_id: &str,
    requester: &str,
    store_dir: &'static StoreDir,
    retry_opts: RetryOpts,
    auth_headers: &AuthHeaders,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
    progress_key: Option<(&SharedReportedProgressKeys, &str)>,
) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
    let mut attempt: u32 = 0;
    loop {
        let result = fetch_and_extract_once::<Reporter>(
            http_client,
            package_url,
            expected_integrity,
            package_unpacked_size,
            download_priority,
            package_id,
            attempt,
            store_dir,
            auth_headers,
            ignore_file_pattern.clone(),
        )
        .await;
        match result {
            Ok(value) => {
                // `pnpm:progress fetched` mirrors pnpm's emit at
                // <https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/package-requester/src/packageRequester.ts#L435>:
                // one event per (resolved) package once the tarball
                // has been pulled from the network and extracted.
                emit_progress_fetched::<Reporter>(package_id, requester, progress_key);
                return Ok(value);
            }
            Err(err) if !is_transient_error(&err) => return Err(err),
            Err(err) if attempt >= retry_opts.retries => {
                tracing::warn!(
                    target: "pacquet::download",
                    ?package_url,
                    attempts = attempt + 1,
                    ?err,
                    "Tarball fetch retry budget exhausted",
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
                    "Tarball fetch failed; retrying after backoff",
                );
                // `pnpm:request-retry` mirrors pnpm's emit at
                // <https://github.com/pnpm/pnpm/blob/086c5e91e8/fetching/tarball-fetcher/src/remoteTarballFetcher.ts#L91>:
                // one event per failed-and-being-retried HTTP
                // attempt, before the backoff sleep, so the JS
                // reporter renders "Will retry in <ms>. <N> retries
                // left." while pacquet is still waiting. `attempt`
                // is one-indexed (the failed attempt) to match
                // pnpm's wire shape; pacquet's loop counter is
                // zero-indexed.
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

impl<'a> DownloadTarballToStore<'a> {
    /// Execute the subroutine with an in-memory cache.
    ///
    /// # Caller invariant: stable filter per URL
    ///
    /// The mem cache is keyed solely by `package_url` (the same
    /// shape as pnpm's `tarballCache` / `archiveCache`), so two
    /// callers fetching the same URL with *different*
    /// [`ignore_file_pattern`] values would receive the same
    /// `cas_paths` map — the one the first caller's filter
    /// produced. Callers must ensure that every fetch of a given
    /// URL uses the same filter.
    ///
    /// In practice this holds because tarball URLs encode
    /// `(name, version, integrity)` and the filter is keyed by
    /// `pkg.name` upstream (`archiveFilters` in
    /// [`binary-fetcher/src/index.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/fetching/binary-fetcher/src/index.ts)),
    /// so the (URL, filter) relation is functional. The dispatcher
    /// in Slice D constructs filters from the same per-package
    /// table; nothing else calls this method with a non-`None`
    /// filter.
    ///
    /// [`ignore_file_pattern`]: DownloadTarballToStore::ignore_file_pattern
    pub async fn run_with_mem_cache<Reporter: self::Reporter>(
        self,
        mem_cache: &'a MemCache,
    ) -> Result<Arc<HashMap<String, PathBuf>>, TarballError> {
        let &DownloadTarballToStore {
            package_url,
            package_id,
            package_integrity,
            prefetched_cas_paths,
            requester,
            ..
        } = &self;
        let cache_key = store_index_key(&package_integrity.to_string(), package_id);
        let progress_key =
            self.progress_reported.as_ref().map(|reported| (reported, cache_key.as_str()));

        // Warm-cache fast path: when [`prefetch_cas_paths`] already
        // batched the `(integrity, pkg_id)` row in at install start,
        // return the `Arc<HashMap>` straight through instead of
        // calling [`Self::run_without_mem_cache`] (which clones the
        // inner per-file map by value before wrapping it in a fresh
        // `Arc`). On warm installs every snapshot lands here; the
        // deep clone the previous path was paying — entire
        // `HashMap<String, PathBuf>` per snapshot, where each entry
        // is a `String` + `PathBuf` allocation — adds up to dominant
        // memory traffic by 1k+ snapshots.
        //
        // Also stash the `Arc` into `mem_cache` keyed by URL so a
        // second fetch of the same tarball (e.g. peer-resolved
        // variants of the same package) hits the in-memory cache
        // without re-checking the prefetched map. Matches what the
        // normal path does with the result of
        // [`Self::run_without_mem_cache`].
        if let Some(prefetched) = prefetched_cas_paths
            && let Some(cas_paths) = prefetched.get(&cache_key)
        {
            tracing::info!(
                target: "pacquet::download",
                ?package_url,
                ?package_id,
                "Reusing prefetched CAFS entry — skipping download (warm-cache fast path)",
            );
            emit_progress_found_in_store::<Reporter>(package_id, requester, progress_key);
            let cas_paths = Arc::clone(cas_paths);
            let cache_lock = Arc::new(RwLock::new(CacheValue::Available(Arc::clone(&cas_paths))));
            mem_cache.insert(package_url.to_string(), cache_lock);
            return Ok(cas_paths);
        }

        // QUESTION: I see no copying from existing store_dir, is there such mechanism?
        // TODO: If it's not implemented yet, implement it

        // `DashMap::get` returns a `Ref` that holds a shard read guard for
        // its entire lifetime. Holding it across `.await` deadlocks: while
        // this task is parked, another task on the same worker can call
        // `mem_cache.insert` for a key that hashes to the same shard,
        // block on the write side, and starve every worker. Clone the
        // inner `Arc` out and drop the `Ref` immediately.
        let existing = mem_cache.get(package_url).map(|entry| Arc::clone(entry.value()));
        if let Some(cache_lock) = existing {
            // `pnpm:progress` fires exactly once per URL — only the
            // first writer's `run_without_mem_cache` call emits.
            // Mirrors pnpm's
            // [`packageRequester`](https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/package-requester/src/packageRequester.ts#L410-L436),
            // which attaches the emit via `.then()` on the first
            // writer's promise; later `await`s of the same promise
            // do not re-trigger the handler.
            //
            // Read-lock the state read: the variant inspection below
            // doesn't mutate anything, and a `write().await` would
            // serialize every late visitor for a popular tarball
            // (e.g. dozens of peer-suffix variants of the same
            // package) behind a single exclusive guard, even though
            // they're all just observing the in-progress / available
            // flag. The owner branch below is the only writer; the
            // RwLock's reader-writer fairness guarantees the owner
            // still makes progress.
            let notify = match &*cache_lock.read().await {
                CacheValue::Available(cas_paths) => {
                    // The first owner already reported its package
                    // status. If the caller supplied a shared
                    // progress set, this emit is skipped for keys the
                    // owner reported; otherwise preserve the legacy
                    // per-caller cache-hit progress.
                    emit_progress_found_in_store::<Reporter>(package_id, requester, progress_key);
                    return Ok(Arc::clone(cas_paths));
                }
                CacheValue::InProgress(notify) => Arc::clone(notify),
                CacheValue::Failed => {
                    // The owner already finished and failed; surface
                    // immediately rather than parking on the Notify.
                    return Err(TarballError::SiblingFetchFailed { url: package_url.to_string() });
                }
            };

            tracing::info!(target: "pacquet::download", ?package_url, "Wait for cache");
            notify.notified().await;
            match &*cache_lock.read().await {
                CacheValue::Available(cas_paths) => {
                    // Same rationale as the pre-notify `Available`
                    // branch above.
                    emit_progress_found_in_store::<Reporter>(package_id, requester, progress_key);
                    Ok(Arc::clone(cas_paths))
                }
                CacheValue::Failed => {
                    Err(TarballError::SiblingFetchFailed { url: package_url.to_string() })
                }
                // The owner only flips us to `Available` or `Failed`
                // before notifying. Hitting this is a programmer
                // error in the owner branch below.
                CacheValue::InProgress(_) => unreachable!(
                    "owner notified waiters but left the cache in InProgress for {package_url:?}",
                ),
            }
        } else {
            let notify = Arc::new(Notify::new());
            let cache_lock = notify
                .pipe_ref(Arc::clone)
                .pipe(CacheValue::InProgress)
                .pipe(RwLock::new)
                .pipe(Arc::new);
            if mem_cache.insert(package_url.to_string(), Arc::clone(&cache_lock)).is_some() {
                tracing::warn!(target: "pacquet::download", ?package_url, "Race condition detected when writing to cache");
            }

            // Run the actual fetch and cleanup in either branch. On
            // error the cache slot must transition to `Failed` and
            // we must `notify_waiters` so concurrent requesters
            // wake up and surface a sibling-fetch error instead of
            // parking on the Notify forever (the original deadlock).
            // Removing the `mem_cache` entry afterwards lets a
            // freshly-started fetch (e.g., via `pacquet add` after
            // a transient network failure) retry without inheriting
            // the failed slot.
            let result = self.run_without_mem_cache::<Reporter>().await;
            match result {
                Ok(cas_paths) => {
                    let cas_paths = Arc::new(cas_paths);
                    let mut cache_write = cache_lock.write().await;
                    *cache_write = CacheValue::Available(Arc::clone(&cas_paths));
                    drop(cache_write);
                    notify.notify_waiters();
                    Ok(cas_paths)
                }
                Err(err) => {
                    let mut cache_write = cache_lock.write().await;
                    *cache_write = CacheValue::Failed;
                    drop(cache_write);
                    mem_cache.remove(package_url);
                    notify.notify_waiters();
                    Err(err)
                }
            }
        }
    }

    /// Execute the subroutine without an in-memory cache.
    pub async fn run_without_mem_cache<Reporter: self::Reporter>(
        &self,
    ) -> Result<HashMap<String, PathBuf>, TarballError> {
        let &DownloadTarballToStore {
            http_client,
            store_dir,
            package_integrity,
            package_unpacked_size,
            package_file_count,
            package_url,
            package_id,
            requester,
            verify_store_integrity,
            prefetched_cas_paths,
            retry_opts,
            auth_headers,
            ..
        } = self;
        let store_index = self.store_index.clone();
        let store_index_writer = self.store_index_writer.clone();
        let verified_files_cache = Arc::clone(&self.verified_files_cache);
        // `Option<Arc<IgnoreEntryFilter>>` isn't `Copy`, so it can't
        // ride along in the deref-destructure above. `.clone()`
        // here bumps the Arc refcount — cheap, and the trait
        // object is shared with the install dispatcher that
        // owns the original.
        let ignore_file_pattern = self.ignore_file_pattern.clone();

        // Before hitting the network, check the SQLite store index: if the
        // tarball is already in the CAFS we can reuse its per-file paths
        // and skip the download entirely. This is the payoff of the v11
        // store migration (<https://github.com/pnpm/pacquet/issues/244>) — pnpm and pacquet share `index.db`, so a
        // previous install of the same (integrity, pkg_id) pair leaves an
        // entry we can read back here.
        //
        // The lookup is best-effort. A missing `index.db`, a missing row,
        // an undecodable entry, or any CAFS file that has gone missing
        // from disk all fall through to the download path below.
        let cache_key = store_index_key(&package_integrity.to_string(), package_id);
        let progress_key =
            self.progress_reported.as_ref().map(|reported| (reported, cache_key.as_str()));
        // Hot path on warm installs: the install-scoped `prefetch_cas_paths`
        // task already ran one batched SELECT + integrity-check pass for
        // every (integrity, pkg_id) the lockfile mentions. If our key is
        // there, the per-snapshot future skips both the SQLite round-trip
        // and the per-file stat work.
        //
        // We still deep-clone the inner per-file `HashMap` here because
        // `run_without_mem_cache` returns an owned `HashMap<..., ...>`;
        // `(**cas_paths).clone()` walks every entry and clones each
        // `String`/`PathBuf`, not the `Arc`. The Arc wrapping in
        // `PrefetchedCasPaths` is what saves the deep clone on the *new*
        // warm-batch path in `create_virtual_store::run` (which uses
        // `cas_paths.as_ref()` to borrow the inner map directly); this
        // fallback path is the per-snapshot tokio-future flow which
        // only fires for cache-miss snapshots, where the deep clone
        // cost is dwarfed by the cold download that would otherwise
        // run. Propagating the `Arc` through this signature would
        // require a wider refactor of `DownloadTarballToStore`'s
        // return type.
        if let Some(prefetched) = prefetched_cas_paths
            && let Some(cas_paths) = prefetched.get(&cache_key)
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
        if let Some(cas_paths) = load_cached_cas_paths(
            store_index,
            store_dir,
            cache_key.clone(),
            verify_store_integrity,
            verified_files_cache,
        )
        .await
        {
            tracing::info!(target: "pacquet::download", ?package_url, ?package_id, "Reusing cached CAFS entry — skipping download");
            emit_progress_found_in_store::<Reporter>(package_id, requester, progress_key);
            return Ok(cas_paths);
        }

        // Offline-mode gate: both cache lookups missed. Upstream pnpm
        // gates only its metadata path on `--offline`; pacquet has no
        // metadata path on the frozen-install flow, so the gate lands
        // here. Error rather than fall through to the network — same
        // shape as upstream's `ERR_PNPM_NO_OFFLINE_META`, scoped to
        // tarballs because that's what pacquet's frozen install needs
        // network for.
        if self.offline {
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

        // Run the full fetch + integrity + extract pipeline under
        // pnpm's retry policy. Mirrors
        // [`remoteTarballFetcher.ts`](https://github.com/pnpm/pnpm/blob/1819226b51/fetching/tarball-fetcher/src/remoteTarballFetcher.ts):
        // a single retried closure wraps both the network side and the
        // `addFilesFromTarball` side, so a flaky transfer that survives
        // TCP framing but fails the SHA-512 hash or trips gzip / tar
        // parsing recovers via re-fetch instead of aborting the install
        // (<https://github.com/pnpm/pacquet/issues/259>). Only HTTP 401 / 403 / 404 fail fast — see
        // [`is_transient_error`].
        let (_computed_integrity, cas_paths, pkg_files_idx) =
            fetch_and_extract_with_retry::<Reporter>(
                http_client,
                package_url,
                Some(package_integrity),
                package_unpacked_size,
                download_priority(package_unpacked_size, package_file_count),
                package_id,
                requester,
                store_dir,
                retry_opts,
                auth_headers,
                ignore_file_pattern,
                progress_key,
            )
            .await?;

        // Hand the per-tarball files index off to the shared writer task
        // from <https://github.com/pnpm/pacquet/pull/265> *after* the retry loop returns, so transient failures
        // don't queue a half-built row that a successful retry would
        // duplicate. `queue` is a non-blocking `UnboundedSender::send`;
        // the writer task owns one connection and batches whatever it
        // drains in one `BEGIN IMMEDIATE; ... ; COMMIT`. `None` means the
        // writer failed to open or the caller handed us none — the row
        // is dropped with a `warn!` and the next install misses on this
        // cache key, matching the read path's stance.
        let index_key = cache_key;
        if let Some(writer) = store_index_writer {
            writer.queue(index_key, pkg_files_idx);
        } else {
            tracing::warn!(
                target: "pacquet::download",
                ?index_key,
                "no shared store-index writer; skipping index row for this tarball",
            );
        }

        Ok(cas_paths)
    }
}

/// Outcome of [`FetchTarballForResolution::run`]: the sha512 integrity
/// computed from the downloaded tarball and the bundled manifest read
/// from its `package.json`. The extracted CAFS paths are not returned —
/// they are stashed in the shared [`MemCache`] keyed by URL so the
/// install pass reuses them without re-downloading.
#[derive(Debug)]
pub struct ResolvedTarball {
    pub integrity: Integrity,
    pub manifest: Option<serde_json::Value>,
}

/// Download a remote tarball during *resolution*, compute its sha512
/// integrity, extract it to the store, and read its bundled manifest.
///
/// Remote (non-registry) https-tarball direct dependencies carry no
/// name/version/integrity at resolve time — those live in the tarball's
/// `package.json`. pnpm learns them in `packageRequester` after the
/// fetch; pacquet builds the lockfile before the install pass, so the
/// `TarballResolver` must fetch here to fill `manifest` + `integrity`
/// into its `ResolveResult`. Passing a `mem_cache` warms it (keyed by
/// URL) so the install pass's
/// [`DownloadTarballToStore::run_with_mem_cache`] reuses the extraction
/// without a second download.
pub struct FetchTarballForResolution<'a> {
    pub http_client: &'a ThrottledClient,
    pub store_dir: &'static StoreDir,
    pub store_index_writer: Option<Arc<StoreIndexWriter>>,
    pub package_url: &'a str,
    pub auth_headers: &'a AuthHeaders,
    pub retry_opts: RetryOpts,
}

impl FetchTarballForResolution<'_> {
    pub async fn run<Reporter: self::Reporter>(
        self,
        mem_cache: Option<&MemCache>,
    ) -> Result<ResolvedTarball, TarballError> {
        let FetchTarballForResolution {
            http_client,
            store_dir,
            store_index_writer,
            package_url,
            auth_headers,
            retry_opts,
        } = self;

        // `None` expected-integrity → compute it from the bytes. The
        // package_id / requester are the post-redirect URL: the real
        // `name@version` is only known once the manifest is read below,
        // and the resolve-time fetch is silent (the install pass owns
        // the reporter ordering), so the placeholder never surfaces.
        // `UNPRIORITIZED`: this fetch gates the resolver's walk (a
        // tarball dep's manifest comes from its archive), so like a
        // packument fetch it must not queue behind sized downloads.
        let (integrity, cas_paths, pkg_files_idx) = fetch_and_extract_with_retry::<Reporter>(
            http_client,
            package_url,
            None,
            None,
            UNPRIORITIZED,
            package_url,
            package_url,
            store_dir,
            retry_opts,
            auth_headers,
            None,
            None,
        )
        .await?;

        let manifest = pkg_files_idx.manifest.clone();
        // Scope the store-index row by the package's canonical
        // `name@version`, matching what the install pass derives from
        // the same manifest. Fall back to the URL when the tarball has
        // no usable `package.json` name (degraded, but keeps the row
        // addressable).
        let package_id =
            manifest_package_id(manifest.as_ref()).unwrap_or_else(|| package_url.to_string());

        let index_key = store_index_key(&integrity.to_string(), &package_id);
        if let Some(writer) = store_index_writer {
            writer.queue(index_key, pkg_files_idx);
        } else {
            tracing::warn!(
                target: "pacquet::download",
                ?index_key,
                "no shared store-index writer; skipping index row for this resolve-time tarball",
            );
        }

        if let Some(mem_cache) = mem_cache {
            let cache_lock = Arc::new(RwLock::new(CacheValue::Available(Arc::new(cas_paths))));
            mem_cache.insert(package_url.to_string(), cache_lock);
        }

        Ok(ResolvedTarball { integrity, manifest })
    }
}

/// `name@version` from a bundled manifest, when both fields are present.
fn manifest_package_id(manifest: Option<&serde_json::Value>) -> Option<String> {
    let manifest = manifest?;
    let name = manifest.get("name")?.as_str()?;
    let version = manifest.get("version")?.as_str()?;
    Some(format!("{name}@{version}"))
}

/// Run one full zip-archive fetch attempt: hit the network, drain the
/// body into RAM, verify the integrity hash, then walk the zip and
/// extract every file entry into the CAFS. Mirrors
/// [`fetch_and_extract_once`] one-for-one (same network permit
/// shape, same post-download semaphore gate, same retry-friendly
/// errors) — only the `spawn_blocking` body differs: integrity check
/// then [`extract_zip_entries`] instead of the gzip + tar path.
///
/// Mirrors upstream's `downloadAndUnpackZip` at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/fetching/binary-fetcher/src/index.ts>,
/// but writes directly into the CAS rather than going through a
/// temp dir + `addFilesFromDir` round-trip (pacquet's
/// [`StoreDir::write_cas_file`] is the same content-addressed write
/// `addFilesFromDir` does on each tempdir file).
// 8 arguments — over the default clippy threshold, but each is
// distinct (see the matching note on `fetch_and_extract_zip_with_retry`).
#[allow(
    clippy::too_many_arguments,
    reason = "the parameters are independent install-scoped inputs; bundling them into a struct only moves the same fields into a wrapper"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "arg count is set by upstream pnpm's fetcher signature"
)]
async fn fetch_and_extract_zip_once<Reporter: self::Reporter>(
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

    let client = http_client.acquire_for_url(package_url).await;

    let mut request = client.get(package_url);
    // Match the tarball download path: resolve the per-URL auth
    // header and attach it. Runtime artifacts (Node.js, Bun, Deno)
    // are typically downloaded from public hosts that don't require
    // auth, but a self-hosted mirror behind a token-protected proxy
    // would 401 without this. Keeps parity with pnpm's binary
    // fetcher which goes through the same `fetchFromRegistry` /
    // auth-header plumbing.
    if let Some(value) = auth_headers.for_url(package_url) {
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
/// Same shape as [`fetch_and_extract_with_retry`]: HTTP 401 / 403 /
/// 404 fail fast, every other error retries with exponential
/// backoff until [`RetryOpts::retries`] is exhausted. On success
/// emits `pnpm:progress fetched` once per (resolved) package, same
/// as the tarball path.
// 10 arguments — over the default clippy threshold for the same
// reason `fetch_and_extract_with_retry` is: each is distinct, and
// bundling into a struct would just push the same fields into a
// wrapper.
#[expect(
    clippy::too_many_arguments,
    reason = "arg count is set by upstream pnpm's fetcher signature"
)]
async fn fetch_and_extract_zip_with_retry<Reporter: self::Reporter>(
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

/// Counterpart to [`DownloadTarballToStore`] for zip-archive binary
/// resolutions. Mirrors pnpm's `downloadAndUnpackZip` at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/fetching/binary-fetcher/src/index.ts>:
/// the zip flow downloads the body, verifies the integrity hash,
/// then walks zip entries and writes each to the CAFS — with the
/// `prefix` field stripped from each entry path before the ignore
/// filter and CAS write so the runtime's top-level
/// `node-vX.Y.Z-<platform>-<arch>/` directory doesn't leak into
/// downstream consumers' paths.
///
/// The store-index lookup, prefetch cache reuse, and store-index
/// writer queueing match [`DownloadTarballToStore`] — runtime
/// artifacts share the same `index.db` schema as ordinary npm
/// packages.
#[must_use]
pub struct DownloadZipArchiveToStore<'a> {
    pub http_client: &'a ThrottledClient,
    pub store_dir: &'static StoreDir,
    pub store_index: Option<SharedReadonlyStoreIndex>,
    pub store_index_writer: Option<Arc<StoreIndexWriter>>,
    pub verify_store_integrity: bool,
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
    /// `prefix` field on `pacquet_lockfile::BinaryResolution`. The
    /// zip extractor strips `{prefix}/` from each entry path before
    /// the ignore-filter check and the CAS write, so downstream
    /// consumers see paths relative to the package root rather than
    /// the runtime-version-stamped wrapper directory.
    pub archive_prefix: Option<&'a str>,
    /// See [`DownloadTarballToStore::ignore_file_pattern`] — the
    /// per-fetch archive filter is shared by both archive types.
    pub ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
    /// See [`DownloadTarballToStore::offline`]. Same semantics for
    /// the zip-archive path: when both cache lookups miss and
    /// `offline` is `true`, the fetcher fails with
    /// [`TarballError::NoOfflineTarball`] rather than hitting the
    /// network.
    pub offline: bool,
}

impl DownloadZipArchiveToStore<'_> {
    /// Execute the subroutine without an in-memory cache. Mirrors
    /// [`DownloadTarballToStore::run_without_mem_cache`] — same
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
            prefetched_cas_paths,
            retry_opts,
            auth_headers,
            archive_prefix,
            ..
        } = self;
        let store_index = self.store_index.clone();
        let store_index_writer = self.store_index_writer.clone();
        let verified_files_cache = Arc::clone(&self.verified_files_cache);
        // See the matching note in
        // [`DownloadTarballToStore::run_without_mem_cache`]: the
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
        if let Some(cas_paths) = load_cached_cas_paths(
            store_index,
            store_dir,
            cache_key,
            verify_store_integrity,
            verified_files_cache,
        )
        .await
        {
            tracing::info!(target: "pacquet::download", ?package_url, ?package_id, "Reusing cached CAFS entry — skipping zip download");
            emit_progress_found_in_store::<Reporter>(package_id, requester, None);
            return Ok(cas_paths);
        }

        // Offline-mode gate (zip archive). Same shape as the tarball
        // path above — see the matching comment there for the
        // upstream rationale.
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

        let (cas_paths, pkg_files_idx) = fetch_and_extract_zip_with_retry::<Reporter>(
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

#[cfg(test)]
mod tests;
