//! Tarball decompression and entry extraction into the CAS.

use super::{
    Cow, Cursor, HashMap, IgnoreEntryFilter, IntoParallelRefIterator, MAX_UNTRUSTED_PREALLOC_BYTES,
    ParallelIterator, PathBuf, Read, TarballError, UNIX_EPOCH, cas_write_pool,
};
use pnpm_fs::file_mode;
use pnpm_package_manifest::{
    files_include_install_scripts, manifest_requires_build, parse_manifest_bytes,
};
use pnpm_store_dir::{
    CafsFileInfo, FileHash, PackageFilesIndex, StoreDir, WriteCasFileFromReaderError,
};
use tar::Archive;
use tracing::instrument;
use zune_inflate::{DeflateDecoder, DeflateOptions};

/// Build the buffer the tarball body streams into, pre-sized from the
/// response's `Content-Length` where possible.
///
/// That header is untrusted: a broken or hostile registry can advertise
/// `u64::MAX`, so the size reaches the allocator only through
/// `try_reserve_exact`, and a refusal becomes `TarballTooLarge` rather
/// than the abort an infallible `with_capacity` would take. A chunked
/// response carries no length and starts from an empty growable `Vec`.
pub(crate) fn allocate_tarball_buffer(
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

/// Bound a registry-supplied `dist.unpackedSize` before it reaches
/// zune-inflate, which reserves the hint as an infallible zero-filled
/// `vec![0; hint]` and aborts the process if that allocation fails.
pub(crate) fn bounded_gzip_size_hint(unpacked_size: Option<usize>) -> Option<usize> {
    unpacked_size.map(|size| size.min(MAX_UNTRUSTED_PREALLOC_BYTES))
}

#[instrument(skip(gz_data), fields(gz_data_len = gz_data.len()))]
pub(crate) fn decompress_gzip(
    gz_data: &[u8],
    unpacked_size: Option<usize>,
) -> Result<Vec<u8>, TarballError> {
    let mut options = DeflateOptions::default().set_confirm_checksum(false);

    if let Some(size) = bounded_gzip_size_hint(unpacked_size) {
        options = options.set_size_hint(size);
    }

    DeflateDecoder::new_with_options(gz_data, options)
        .decode_gzip()
        .map_err(TarballError::DecodeGzip)
}

/// Compressed-size pivot for [`should_stream_extract`]. The compressed
/// length is the one exact size we hold in hand; npm tarballs
/// typically inflate ~3-5×, so 16 MiB compressed puts the eager path's
/// whole-archive buffer well past [`MAX_UNTRUSTED_PREALLOC_BYTES`].
pub(crate) const STREAM_EXTRACT_COMPRESSED_THRESHOLD: usize = 16 * 1024 * 1024;

/// Whether a downloaded tarball should be extracted through the
/// streaming path ([`stream_extract_gzipped_tarball`]) instead of the
/// eager whole-archive decompression
/// ([`decompress_gzip`] + [`extract_tarball_entries`]).
///
/// The eager path materializes the entire decompressed archive as one
/// contiguous buffer, which for a large package multiplies across every
/// concurrently extracting task. Stream once either signal says the
/// archive is large: the exact compressed length, or the registry's
/// `dist.unpackedSize` hint. The hint is attacker-controlled, but here
/// it only picks between two correct extraction paths — a lying value
/// costs at most the wrong path's performance profile, never
/// correctness or unbounded memory.
pub(crate) fn should_stream_extract(compressed_len: usize, unpacked_size: Option<usize>) -> bool {
    compressed_len >= STREAM_EXTRACT_COMPRESSED_THRESHOLD
        || unpacked_size.is_some_and(|size| size >= MAX_UNTRUSTED_PREALLOC_BYTES)
}

/// Pick the `package.json` fields downstream code actually reads — bin
/// linking, dependency resolution, build-script detection — and discard
/// the rest, keeping only the three lifecycle hooks pnpm executes out
/// of `scripts`.
///
/// The subset exists to bound what lands in `index.db`: a full manifest
/// runs to tens of KB, and msgpackr-records tops out at `0x7f` record
/// slots (see [`pnpm_store_dir::EncodeError::OutOfRecordSlots`]).
///
/// `None` rather than an empty object when nothing survives, which
/// would otherwise round-trip as a zero-field record def.
pub(crate) fn normalize_bundled_manifest(value: &serde_json::Value) -> Option<serde_json::Value> {
    /// Fields kept verbatim from the source manifest.
    ///
    /// Order matters for the on-wire byte sequence — msgpackr emits
    /// fields in JS object insertion order, and pacquet's encoder
    /// follows the [`serde_json::Map`] iteration order — but it
    /// does *not* matter for property-access correctness on the
    /// pnpm side. The order below matches the field order pnpm
    /// emits so a side-by-side byte diff against a pnpm-written
    /// row is shallower.
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
    // and pulling `node-semver` into `pnpm-tarball` purely for
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
/// cleaned, paired with its payload — a borrow into the decompressed
/// archive buffer on the eager path, an owned copy on the streaming
/// path. Collected serially while walking the tar stream, then hashed
/// and written to the CAFS — serially or across the rayon pool — in
/// [`write_cas_entry`].
pub(crate) struct PendingFile<'a> {
    cleaned_path: String,
    data: Cow<'a, [u8]>,
    executable: bool,
    mode: u32,
    size: u64,
}

/// Hash one [`PendingFile`] into the content-addressed store and build
/// its [`CafsFileInfo`] index row. Pure given the inputs and the store
/// dir's content-addressed layout, so it is safe to run concurrently
/// across entries of the same tarball.
pub(crate) fn write_cas_entry(
    store_dir: &StoreDir,
    file: &PendingFile<'_>,
) -> Result<(String, PathBuf, CafsFileInfo), TarballError> {
    let (file_path, file_hash) = store_dir
        .write_cas_file(&file.data, file.executable)
        .map_err(TarballError::WriteCasFile)?;
    Ok((file.cleaned_path.clone(), file_path, cafs_file_info(&file_hash, file.mode, file.size)))
}

/// Build the [`CafsFileInfo`] index row for a freshly written CAS file.
pub(crate) fn cafs_file_info(file_hash: &FileHash, mode: u32, size: u64) -> CafsFileInfo {
    // `as_millis()` returns `u128`; narrow to `u64` to match the store
    // index schema (see `CafsFileInfo::checked_at`). Drop the timestamp
    // if the clock reports something unrepresentable — `checkedAt` is
    // optional and pnpm tolerates `None`.
    let checked_at =
        UNIX_EPOCH.elapsed().ok().and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok());
    CafsFileInfo { digest: format!("{file_hash:x}"), mode, size, checked_at }
}

/// Fold a synthesized `package.json` (pnpm's `appendManifest`) into a
/// freshly extracted archive's CAFS output. Runtime archives (Node.js /
/// Bun / Deno) carry no `package.json` of their own, so the caller
/// supplies one, and it also becomes the store-index row's bundled
/// `manifest` — which is what lets the warm-batch bin linker find the
/// runtime's bin without a disk round-trip.
///
/// See [`write_synthesized_package_json`] for what reaching the store
/// entails and when the write is skipped.
pub(crate) fn apply_append_manifest(
    store_dir: &StoreDir,
    manifest_bytes: &[u8],
    cas_paths: &mut HashMap<String, PathBuf>,
    pkg_files_idx: &mut PackageFilesIndex,
) -> Result<(), TarballError> {
    if !write_synthesized_package_json(store_dir, manifest_bytes, cas_paths, pkg_files_idx)? {
        return Ok(());
    }
    // Surface the synthesized manifest as the row's bundled manifest so
    // the warm-batch bin linker reads the bin here instead of stat-ing the
    // slot. Only when the archive supplied none, mirroring pnpm's guard.
    if pkg_files_idx.manifest.is_none()
        && let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(manifest_bytes)
    {
        pkg_files_idx.manifest = normalize_bundled_manifest(&parsed);
    }
    Ok(())
}

/// Give an archive that ships no `package.json` of its own the
/// placeholder one pnpm writes, so every extracted package has one and
/// materialization can treat it as the slot's completion marker.
///
/// The placeholder is a marker, not a manifest: its `_pnpmPlaceholder`
/// field is how a reader tells it apart from a real one, and the
/// store-index row's bundled `manifest` stays empty so nothing mistakes
/// it for the package's identity.
///
/// See [`write_synthesized_package_json`] for what reaching the store
/// entails and when the write is skipped — a real `package.json`,
/// including one [`apply_append_manifest`] just synthesized, always
/// takes precedence.
pub(crate) fn apply_placeholder_manifest(
    store_dir: &StoreDir,
    cas_paths: &mut HashMap<String, PathBuf>,
    pkg_files_idx: &mut PackageFilesIndex,
) -> Result<(), TarballError> {
    write_synthesized_package_json(store_dir, PLACEHOLDER_PACKAGE_JSON, cas_paths, pkg_files_idx)?;
    Ok(())
}

/// The `package.json` pnpm writes for a package that genuinely has none.
/// The `_pnpmPlaceholder` field tells a manifest reader to ignore it.
pub(crate) const PLACEHOLDER_PACKAGE_JSON: &[u8] = br#"{"_pnpmPlaceholder":"This file was generated by pnpm. The original package did not contain a package.json."}"#;

/// Write `bytes` into the content-addressed store as the archive's
/// `package.json`, recording it in both `cas_paths` (this install's
/// slot) and the persisted `pkg_files_idx`. Baking the file into the
/// store-index row is what lets a later warm materialization land a
/// `package.json` slot without re-extracting.
///
/// Returns whether anything was written — `false` when the archive
/// already carries a `package.json`, which always wins.
pub(crate) fn write_synthesized_package_json(
    store_dir: &StoreDir,
    bytes: &[u8],
    cas_paths: &mut HashMap<String, PathBuf>,
    pkg_files_idx: &mut PackageFilesIndex,
) -> Result<bool, TarballError> {
    if pkg_files_idx.files.contains_key("package.json") {
        return Ok(false);
    }
    let (cas_path, file_hash) =
        store_dir.write_cas_file(bytes, false).map_err(TarballError::WriteCasFile)?;
    let checked_at =
        UNIX_EPOCH.elapsed().ok().and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok());
    let info = CafsFileInfo {
        digest: format!("{file_hash:x}"),
        // A synthesized manifest is a plain, non-executable data file;
        // `0o644` is the same canonical mode `add_files_from_dir` reports
        // for a non-executable entry (and pnpm's Windows-host default).
        mode: 0o644,
        size: bytes.len() as u64,
        checked_at,
    };
    cas_paths.insert("package.json".to_string(), cas_path);
    pkg_files_idx.files.insert("package.json".to_string(), info);
    Ok(true)
}

/// Walk decompressed tar bytes, writing each regular-file entry into
/// the CAFS and returning the `{in-tarball path → CAFS path}` map plus
/// the per-tarball [`PackageFilesIndex`] row to hand off to the shared
/// store-index writer.
///
/// Only regular files are stored; a published npm tarball carries
/// nothing else that pacquet can represent.
///
/// The archive is already fully buffered in memory by the download
/// pipeline. Use `entries_with_seek` + `raw_file_position` to borrow
/// each file payload as a slice of that buffer instead of allocating a
/// fresh `Vec<u8>` and `read_to_end`-ing every entry.
///
/// Every tar-side failure comes back as
/// [`TarballError::ReadTarballEntries`] instead of panicking, and a
/// non-UTF-8 entry path is coerced via
/// [`std::path::Path::to_string_lossy`] to match pnpm's string-based
/// handling, so a mixed install against the shared `index.db` agrees.
pub(crate) fn extract_tarball_entries(
    tar_data: &[u8],
    store_dir: &StoreDir,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
) -> Result<(HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
    let mut archive = Archive::new(Cursor::new(tar_data));
    let entries = archive
        .entries_with_seek()
        .map_err(TarballError::ReadTarballEntries)?
        // `Err` entries pass the filter so the `?` below propagates
        // them rather than silently dropping a malformed archive.
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
    let mut manifest_build_scripts = false;
    let mut file_build_hooks = false;

    for entry in entries {
        let entry = entry.map_err(TarballError::ReadTarballEntries)?;

        let file_mode = entry.header().mode().map_err(TarballError::ReadTarballEntries)?;
        let file_is_executable = file_mode::is_executable(file_mode);
        let file_size = entry.header().size().map_err(TarballError::ReadTarballEntries)?;
        let entry_data = tar_entry_payload(tar_data, &entry)?;

        let entry_path = entry.path().map_err(TarballError::ReadTarballEntries)?;
        let cleaned_entry_path = clean_archive_entry_path(&entry_path.to_string_lossy())?;
        // Drop ignored entries before the CAS write. Paths are matched
        // *after* the top-level prefix strip, so the callback sees the
        // cleaned relative path. Bypassing the CAS write here also
        // keeps the package's [`PackageFilesIndex`] tight — an ignored
        // entry never surfaces in `files` or `manifest`.
        if let Some(filter) = ignore_file_pattern
            && filter(&cleaned_entry_path)
        {
            continue;
        }
        if files_include_install_scripts([cleaned_entry_path.as_str()]) {
            file_build_hooks = true;
        }
        if cleaned_entry_path == "package.json" {
            (manifest_build_scripts, manifest) = capture_bundled_manifest(entry_data);
        }

        pending.push(PendingFile {
            cleaned_path: cleaned_entry_path,
            data: Cow::Borrowed(entry_data),
            executable: file_is_executable,
            mode: file_mode,
            size: file_size,
        });
    }

    let written = write_pending_files(store_dir, &pending)?;
    Ok(assemble_extract_output(written, manifest, manifest_build_scripts || file_build_hooks))
}

/// Hash and write a slice of pending files into the content-addressed
/// store, preserving input order in the returned rows.
///
/// Extracting a package with thousands of files (e.g. `core-js`) on a
/// single blocking thread pins one core while the rest sit idle — most
/// costly at the makespan tail, when it's the last extraction still
/// running. [`write_cas_entry`] is safe to run concurrently, so large
/// slices fan out across the dedicated [`cas_write_pool`]; small ones
/// stay serial to skip rayon's per-job dispatch cost when there's
/// nothing to gain. The dedicated pool keeps this off the global pool
/// the linker uses, so an extraction burst can't stall `node_modules`
/// linking running concurrently.
fn write_pending_files(
    store_dir: &StoreDir,
    pending: &[PendingFile<'_>],
) -> Result<Vec<(String, PathBuf, CafsFileInfo)>, TarballError> {
    const PARALLEL_EXTRACT_THRESHOLD: usize = 32;
    if pending.len() >= PARALLEL_EXTRACT_THRESHOLD {
        let write_all = || -> Result<Vec<(String, PathBuf, CafsFileInfo)>, TarballError> {
            pending.par_iter().map(|file| write_cas_entry(store_dir, file)).collect()
        };
        match cas_write_pool() {
            Some(pool) => pool.install(write_all),
            None => write_all(),
        }
    } else {
        pending.iter().map(|file| write_cas_entry(store_dir, file)).collect()
    }
}

/// Assemble the extraction outputs from written CAS rows. `written`
/// preserves entry order, so a tarball with duplicate paths keeps the
/// last entry — matching pnpm's last-wins `filesIndex.set`.
fn assemble_extract_output(
    written: Vec<(String, PathBuf, CafsFileInfo)>,
    manifest: Option<serde_json::Value>,
    requires_build: bool,
) -> (HashMap<String, PathBuf>, PackageFilesIndex) {
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
        requires_build: Some(requires_build),
        algo: "sha512".to_string(),
        files,
        side_effects: None,
    };
    (cas_paths, pkg_files_idx)
}

/// Parse a tarball's bundled `package.json`, returning its
/// requires-build flag and the narrowed manifest for the store-index
/// row.
///
/// The narrowed manifest is stashed in `pkgFilesIndex.manifest` so
/// install-side consumers (notably bin linking) can avoid re-reading
/// the file from disk — the same place pnpm keeps it, so the shared
/// `index.db` row carries it for both tools. The
/// [`normalize_bundled_manifest`] pick drops fields downstream code
/// doesn't use, keeping `index.db` rows tight.
///
/// Callers apply this to every `package.json` entry they see, so a
/// duplicate entry overwrites any earlier one and the final entry is
/// canonical — same shape as the `files` map, which already overwrites
/// duplicates. Real npm tarballs never publish multiple `package.json`
/// entries, but the consistency with the `files` map is what matters:
/// `manifest` and `files` must describe the same file.
///
/// Failed JSON parses degrade to `(false, None)` — the manifest is
/// best-effort; a corrupt `package.json` is the publisher's fault and
/// downstream code can fall back to disk reads.
fn capture_bundled_manifest(entry_data: &[u8]) -> (bool, Option<serde_json::Value>) {
    match parse_manifest_bytes(entry_data) {
        Ok(parsed) => (manifest_requires_build(&parsed), normalize_bundled_manifest(&parsed)),
        Err(error) => {
            tracing::debug!(
                ?error,
                "package.json in tarball failed to parse as JSON; bundled manifest cleared",
            );
            (false, None)
        }
    }
}

/// Validate and clean one archive entry path: reject traversal, drop
/// the top-level package directory (`package/`), and join the remaining
/// segments with forward slashes.
///
/// Rejected rather than normalized so a tampered tarball is visible
/// instead of silently landing outside the store.
///
/// Joined by hand rather than with `PathBuf`, whose native separator
/// would desynchronize these keys from pnpm's always-forward-slashed
/// path layer and the `index.db` both implementations share. Callers
/// pass the `to_string_lossy` rendering, which coerces non-UTF-8 bytes
/// to U+FFFD per component.
fn clean_archive_entry_path(raw: &str) -> Result<String, TarballError> {
    let Some(mut parts) = archive_entry_segments(raw) else {
        return Err(TarballError::ReadTarballEntries(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "tar entry path rejected (non-normal component, possible directory traversal): {raw:?}",
            ),
        )));
    };
    parts.remove(0);
    if parts.is_empty() {
        return Err(TarballError::ReadTarballEntries(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "tar entry path has no payload after dropping the top-level component: {raw:?}",
            ),
        )));
    }
    Ok(parts.join("/"))
}

/// Ceiling for buffering one tar entry in memory on the streaming
/// extraction path. Entries at or below this go through the batched
/// [`write_pending_files`] fan-out; larger ones stream straight into
/// the store via [`StoreDir::write_cas_file_from_reader`] without ever
/// being held in memory. `package.json` is the exception — the bundled
/// manifest must be parsed from bytes (build-script detection depends
/// on it), so it is buffered up to [`MAX_UNTRUSTED_PREALLOC_BYTES`],
/// beyond which the archive is rejected as hostile.
pub(crate) const STREAM_ENTRY_BUFFER_MAX: u64 = 4 * 1024 * 1024;

/// Byte budget for one batch of buffered entries on the streaming
/// extraction path. A batch flushes to [`write_pending_files`] once it
/// holds this much payload, so peak memory stays bounded by the budget
/// (plus one in-flight entry) instead of the archive's unpacked size,
/// while typical batches are still large enough for the parallel
/// CAS-write fan-out to pay off.
const STREAM_BATCH_BUDGET_BYTES: usize = 32 * 1024 * 1024;

/// Decompress and extract a gzipped tarball without materializing the
/// decompressed archive: [`extract_tarball_entries_streaming`] over a
/// streaming gzip decoder.
///
/// The eager [`decompress_gzip`] + [`extract_tarball_entries`] pair
/// stays the default for small tarballs, where the whole-archive
/// buffer is cheap and buys zero-copy payload slices plus one big
/// parallel write phase; [`should_stream_extract`] decides which path
/// a download takes.
pub(crate) fn stream_extract_gzipped_tarball(
    gz_data: &[u8],
    store_dir: &StoreDir,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
) -> Result<(HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
    extract_tarball_entries_streaming(
        flate2::read::GzDecoder::new(gz_data),
        store_dir,
        ignore_file_pattern,
    )
}

/// Walk a tar stream, writing each regular-file entry into the CAFS
/// and returning the same outputs as [`extract_tarball_entries`],
/// while holding only a bounded window of the archive in memory.
///
/// Small entries are buffered and flushed in bounded batches through
/// the same parallel write phase as the eager path; entries above
/// [`STREAM_ENTRY_BUFFER_MAX`] stream straight into the store with an
/// incremental hash. A batch always flushes before a streamed entry is
/// written, so the output rows keep archive order and the last-wins
/// duplicate semantics of [`assemble_extract_output`] hold.
///
/// Decoder failures (e.g. a corrupt gzip stream) surface through the
/// reader as [`TarballError::ReadTarballEntries`]; the retry
/// classifier treats them the same as an eager-path decode error.
pub(crate) fn extract_tarball_entries_streaming(
    reader: impl Read,
    store_dir: &StoreDir,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
) -> Result<(HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
    let truncated = || {
        TarballError::ReadTarballEntries(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "tar entry payload extends beyond archive",
        ))
    };

    let mut archive = Archive::new(reader);
    let mut written: Vec<(String, PathBuf, CafsFileInfo)> = Vec::new();
    let mut batch: Vec<PendingFile<'static>> = Vec::new();
    let mut batch_bytes: usize = 0;
    let mut manifest = None;
    let mut manifest_build_scripts = false;
    let mut file_build_hooks = false;

    for entry in archive.entries().map_err(TarballError::ReadTarballEntries)? {
        let mut entry = entry.map_err(TarballError::ReadTarballEntries)?;
        if !entry.header().entry_type().is_file() {
            continue;
        }

        let file_mode = entry.header().mode().map_err(TarballError::ReadTarballEntries)?;
        let file_is_executable = file_mode::is_executable(file_mode);
        let file_size = entry.header().size().map_err(TarballError::ReadTarballEntries)?;
        let cleaned_entry_path = {
            let entry_path = entry.path().map_err(TarballError::ReadTarballEntries)?;
            clean_archive_entry_path(&entry_path.to_string_lossy())?
        };
        // Same drop-before-the-CAS-write semantics as the eager loop:
        // an ignored entry never surfaces in `files` or `manifest`.
        if let Some(filter) = ignore_file_pattern
            && filter(&cleaned_entry_path)
        {
            continue;
        }
        if files_include_install_scripts([cleaned_entry_path.as_str()]) {
            file_build_hooks = true;
        }

        // `package.json` must be buffered — the bundled manifest and
        // its build-script detection are parsed from bytes, exactly as
        // on the eager path — so its size is hard-capped instead:
        // buffering an unbounded manifest would defeat this path's
        // bounded-memory guarantee, and silently skipping the parse
        // would record wrong build metadata. A manifest beyond the cap
        // exists only in a hostile archive (real ones are a few KB), so
        // failing loudly is the honest outcome. A tar entry's payload
        // can never exceed its header size, so the pre-read check is
        // sufficient.
        if cleaned_entry_path == "package.json" && file_size > MAX_UNTRUSTED_PREALLOC_BYTES as u64 {
            return Err(TarballError::ReadTarballEntries(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "tar entry package.json is {file_size} bytes, which exceeds the \
                     {MAX_UNTRUSTED_PREALLOC_BYTES}-byte manifest limit",
                ),
            )));
        }
        let buffer_entry =
            file_size <= STREAM_ENTRY_BUFFER_MAX || cleaned_entry_path == "package.json";
        if buffer_entry {
            let mut data = Vec::with_capacity(file_size as usize);
            entry.read_to_end(&mut data).map_err(TarballError::ReadTarballEntries)?;
            if data.len() as u64 != file_size {
                return Err(truncated());
            }
            if cleaned_entry_path == "package.json" {
                (manifest_build_scripts, manifest) = capture_bundled_manifest(&data);
            }
            batch_bytes += data.len();
            batch.push(PendingFile {
                cleaned_path: cleaned_entry_path,
                data: Cow::Owned(data),
                executable: file_is_executable,
                mode: file_mode,
                size: file_size,
            });
            if batch_bytes >= STREAM_BATCH_BUDGET_BYTES {
                flush_pending_batch(store_dir, &mut batch, &mut batch_bytes, &mut written)?;
            }
        } else {
            flush_pending_batch(store_dir, &mut batch, &mut batch_bytes, &mut written)?;
            // `Some(file_size)` makes the store writer reject a short
            // stream before anything is committed to a
            // content-addressed path, so a truncated archive leaves no
            // orphan blob behind.
            let (file_path, file_hash, streamed_size) = store_dir
                .write_cas_file_from_reader(&mut entry, file_is_executable, Some(file_size))
                .map_err(|error| match error {
                    WriteCasFileFromReaderError::Read(error) => {
                        TarballError::ReadTarballEntries(error)
                    }
                    WriteCasFileFromReaderError::Write(error) => TarballError::WriteCasFile(error),
                })?;
            written.push((
                cleaned_entry_path,
                file_path,
                cafs_file_info(&file_hash, file_mode, streamed_size),
            ));
        }
    }
    flush_pending_batch(store_dir, &mut batch, &mut batch_bytes, &mut written)?;

    Ok(assemble_extract_output(written, manifest, manifest_build_scripts || file_build_hooks))
}

/// Hash and write the buffered batch into the CAFS, appending its rows
/// to `written` in order and resetting the batch accumulator.
fn flush_pending_batch(
    store_dir: &StoreDir,
    batch: &mut Vec<PendingFile<'_>>,
    batch_bytes: &mut usize,
    written: &mut Vec<(String, PathBuf, CafsFileInfo)>,
) -> Result<(), TarballError> {
    if batch.is_empty() {
        return Ok(());
    }
    written.extend(write_pending_files(store_dir, batch)?);
    batch.clear();
    *batch_bytes = 0;
    Ok(())
}

/// Borrow one tar entry's payload out of the decompressed archive.
///
/// The tar reader is seekable over an in-memory buffer, so an entry's
/// bytes are already there — slicing them costs nothing, where reading
/// through the entry would copy every payload into a fresh allocation
/// sized by the archive's own (untrusted) header.
///
/// The bounds are all checked: a header whose offset or size doesn't fit
/// a `usize`, whose sum overflows, or whose range runs past the end of
/// the archive is rejected rather than truncated.
pub(crate) fn tar_entry_payload<'a, Reader: std::io::Read>(
    tar_data: &'a [u8],
    entry: &tar::Entry<'_, Reader>,
) -> Result<&'a [u8], TarballError> {
    let invalid = |message: &str| {
        TarballError::ReadTarballEntries(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            message.to_string(),
        ))
    };
    let file_size = entry.header().size().map_err(TarballError::ReadTarballEntries)?;
    let data_offset = usize::try_from(entry.raw_file_position())
        .map_err(|_| invalid("tar entry file offset does not fit in usize"))?;
    let size = usize::try_from(file_size)
        .map_err(|_| invalid("tar entry file size does not fit in usize"))?;
    let end = data_offset
        .checked_add(size)
        .ok_or_else(|| invalid("tar entry file offset plus size overflows usize"))?;
    tar_data.get(data_offset..end).ok_or_else(|| {
        TarballError::ReadTarballEntries(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "tar entry payload extends beyond archive",
        ))
    })
}

/// Split a published archive entry's path into its segments, rejecting
/// anything that escapes the archive root.
///
/// `\` is treated as a separator, as pnpm does before it validates
/// (`parseTarball.ts`). Without that, a Windows-built entry keeps its
/// backslashes verbatim on Unix — where they are ordinary filename
/// characters — and the resulting key travels through the `index.db`
/// both implementations share to a reader that *does* treat them as
/// separators.
///
/// `None` for an absolute path or one climbing past the root.
pub(crate) fn archive_entry_segments(raw: &str) -> Option<Vec<String>> {
    let normalized = raw.replace('\\', "/");
    if normalized.starts_with('/') {
        return None;
    }
    let mut segments = Vec::new();
    for segment in normalized.split('/') {
        match segment {
            "" | "." => {}
            ".." => return None,
            other => segments.push(other.to_string()),
        }
    }
    (!segments.is_empty()).then_some(segments)
}
