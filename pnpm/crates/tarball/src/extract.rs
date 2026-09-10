//! Tarball decompression and entry extraction into the CAS.

pub(crate) use manifest::{
    apply_append_manifest, apply_placeholder_manifest, normalize_bundled_manifest,
};
pub(crate) use streaming::{
    BodyChunkSender, STREAM_ENTRY_BUFFER_MAX, body_chunk_channel, stream_extract_gzipped_channel,
    stream_extract_gzipped_tarball, tar_entry_payload,
};

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
use zune_inflate::{DeflateDecoder, DeflateOptions, errors::DecodeErrorStatus};

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

/// Bound an untrusted unpacked-size claim — the registry's
/// `dist.unpackedSize` or the archive's own gzip trailer — before it
/// reaches zune-inflate, which reserves the hint as an infallible
/// zero-filled `vec![0; hint]` and aborts the process if that
/// allocation fails.
pub(crate) fn bounded_gzip_size_hint(unpacked_size: Option<usize>) -> Option<usize> {
    unpacked_size.map(|size| size.min(MAX_UNTRUSTED_PREALLOC_BYTES))
}

/// Decompress a whole gzipped archive into one contiguous buffer,
/// refusing to inflate past [`MAX_UNTRUSTED_PREALLOC_BYTES`].
///
/// The ceiling is the same one [`should_stream_extract`] pivots on, so
/// the two agree on how large an archive the eager path may hold — the
/// difference being that this one measures the archive instead of
/// trusting a hint about it. Both signals [`should_stream_extract`] has
/// can be wrong: `dist.unpackedSize` is attacker-controlled, and the
/// compressed length says nothing about the ratio. Integrity
/// verification is no help either, since a gzip bomb is a legitimately
/// published package whose hash matches. Without the ceiling the only
/// bound is `zune-inflate`'s own 1 GiB default, which every
/// concurrently extracting task may claim (see
/// [`crate::post_download_semaphore`]).
///
/// Exceeding it is not a refusal: callers answer
/// [`is_eager_decode_limit_exceeded`] by re-running the archive through
/// a streaming decoder, which decodes it in full.
#[instrument(skip(gz_data), fields(gz_data_len = gz_data.len()))]
pub(crate) fn decompress_gzip(
    gz_data: &[u8],
    unpacked_size: Option<usize>,
) -> Result<Vec<u8>, TarballError> {
    let mut options = DeflateOptions::default()
        .set_confirm_checksum(false)
        .set_limit(MAX_UNTRUSTED_PREALLOC_BYTES);

    if let Some(size) = bounded_gzip_size_hint(unpacked_size) {
        options = options.set_size_hint(size);
    }

    DeflateDecoder::new_with_options(gz_data, options)
        .decode_gzip()
        .map_err(TarballError::DecodeGzip)
}

/// Whether `error` is [`decompress_gzip`] reporting that the archive
/// inflated past its ceiling, the one decode failure that says nothing
/// about the archive being malformed.
pub(crate) fn is_eager_decode_limit_exceeded(error: &TarballError) -> bool {
    matches!(
        error,
        TarballError::DecodeGzip(decode) if matches!(decode.error, DecodeErrorStatus::OutputLimitExceeded(..)),
    )
}

/// Extract a fully buffered gzipped tarball into the CAFS through
/// whichever of the two extractors suits its size.
///
/// Eager extraction buys zero-copy payload slices and one big parallel
/// write phase, and holds the whole decompressed archive to do it —
/// multiplied across every extraction running concurrently. It is
/// therefore taken only while the archive is small:
/// [`should_stream_extract`] routes on the size signals available
/// before decoding, and [`decompress_gzip`]'s ceiling catches an
/// archive that only turns out to be large once it inflates.
///
/// No archive is refused for its size. Both outcomes route to
/// [`stream_extract_gzipped_tarball`], which decodes the same bytes
/// with the same results in bounded memory.
pub(crate) fn extract_gzipped_tarball(
    gz_data: &[u8],
    unpacked_size: Option<usize>,
    store_dir: &StoreDir,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
) -> Result<(HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
    // Route on the larger of the two claims about the unpacked size.
    // Neither is trustworthy, and taking the larger is the conservative
    // reading: a registry hint that under-reports cannot hide a trailer
    // that does not, or the other way round.
    let unpacked_size = unpacked_size.max(gzip_isize_hint(gz_data));
    if should_stream_extract(gz_data.len(), unpacked_size) {
        return stream_extract_gzipped_tarball(gz_data, store_dir, ignore_file_pattern);
    }
    match decompress_gzip(gz_data, unpacked_size) {
        Ok(tar_data) => extract_tarball_entries(&tar_data, store_dir, ignore_file_pattern),
        Err(error) if is_eager_decode_limit_exceeded(&error) => {
            tracing::debug!(
                target: "pacquet::download",
                gz_data_len = gz_data.len(),
                "archive inflated past the eager decode ceiling; extracting it as a stream",
            );
            stream_extract_gzipped_tarball(gz_data, store_dir, ignore_file_pattern)
        }
        Err(error) => Err(error),
    }
}

/// The uncompressed size a gzip stream records in its own trailer.
///
/// The last four bytes of a gzip member are ISIZE: what it decodes to,
/// modulo 2^32. It is the archive's own claim and no more trustworthy
/// than the registry's `dist.unpackedSize` — but it is available where
/// that one often is not (a lockfile records no unpacked size, so a
/// frozen install has nothing else), and routing on it means an honest
/// archive that inflates past the eager ceiling is streamed on the
/// first pass instead of being decoded twice. A dishonest one is still
/// caught by [`decompress_gzip`]'s ceiling.
///
/// `None` unless the buffer opens with a deflate member header — the
/// same three bytes the decoder itself checks first. A body that will
/// fail at the decoder anyway keeps taking the path whose diagnostic
/// says so, rather than being routed by four bytes of whatever it
/// happens to end with.
pub(crate) fn gzip_isize_hint(gz_data: &[u8]) -> Option<usize> {
    if !gz_data.starts_with(&GZIP_MAGIC) || gz_data.get(GZIP_MAGIC.len()) != Some(&GZIP_CM_DEFLATE)
    {
        return None;
    }
    let trailer: [u8; 4] = gz_data.get(gz_data.len().checked_sub(4)?..)?.try_into().ok()?;
    usize::try_from(u32::from_le_bytes(trailer)).ok()
}

/// The decode error a body whose first bytes are not gzip will produce,
/// raised from those bytes alone so a response that cannot be an
/// archive is never buffered in full. `decode_gzip` rejects on the
/// magic number, so the verdict does not depend on how much of the body
/// has arrived.
pub(crate) fn non_gzip_body_error(prefix_len: usize) -> TarballError {
    let status = if prefix_len < GZIP_MAGIC.len() {
        DecodeErrorStatus::InsufficientData
    } else {
        DecodeErrorStatus::CorruptData
    };
    TarballError::DecodeGzip(zune_inflate::errors::InflateDecodeErrors::new_with_error(status))
}

/// First bytes of every gzip member, and all a reader needs to tell an
/// archive from whatever else a server might answer with.
pub(crate) const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// Compression method byte following [`GZIP_MAGIC`]. Deflate is the
/// only method npm archives use and the only one the decoder accepts.
const GZIP_CM_DEFLATE: u8 = 8;

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
/// archive is large: the exact compressed length, or an unpacked-size
/// claim ([`crate::extract_gzipped_tarball`] takes the larger of the
/// registry's `dist.unpackedSize` and the gzip trailer's). A claim is
/// attacker-controlled, but here it only picks between two correct
/// extraction paths — a lying value costs at most the wrong path's
/// performance profile, and [`decompress_gzip`]'s ceiling keeps even
/// that path's memory bounded.
pub(crate) fn should_stream_extract(compressed_len: usize, unpacked_size: Option<usize>) -> bool {
    compressed_len >= STREAM_EXTRACT_COMPRESSED_THRESHOLD
        || unpacked_size.is_some_and(|size| size >= MAX_UNTRUSTED_PREALLOC_BYTES)
}

/// Minimum known compressed size for extracting a registry tarball while its
/// body is still arriving. This reserves long-lived blocking tasks for archives
/// whose post-download extraction is likely to extend the install tail.
pub(crate) const STREAM_EXTRACT_DURING_DOWNLOAD_THRESHOLD: u64 = 4 * 1024 * 1024;

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
        let Some(meta) = entry_meta(&entry, ignore_file_pattern)? else {
            continue;
        };
        let entry_data = tar_entry_payload(tar_data, &entry)?;

        file_build_hooks |= files_include_install_scripts([meta.cleaned_path.as_str()]);
        if meta.cleaned_path == "package.json" {
            (manifest_build_scripts, manifest) = capture_bundled_manifest(entry_data);
        }

        pending.push(PendingFile {
            cleaned_path: meta.cleaned_path,
            data: Cow::Borrowed(entry_data),
            executable: meta.executable,
            mode: meta.mode,
            size: meta.size,
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
        requires_prepare: None,
        algo: "sha512".to_string(),
        files,
        side_effects: None,
        remote_side_effects_quarantine: None,
    };
    (cas_paths, pkg_files_idx)
}

/// Validate and clean one archive entry path: reject traversal, drop
/// the top-level package directory (`package/`), and join the remaining
/// segments with forward slashes.
///
/// Rejected rather than normalized so a tampered tarball is visible
/// instead of silently landing outside the store.
///
/// An entry that is only one segment long keeps that segment. Such an
/// entry sits at the archive root — beside `package/`, or in a flat
/// archive with no wrapping directory at all — so there is no
/// top-level directory on it to drop, and pnpm keys it by its own name
/// (`parseString` in `parseTarball.ts` advances past the first
/// separator, which a single segment has none of). Dropping the segment
/// instead would leave nothing to key the file by, and rejecting the
/// entry would fail an archive that every other installer accepts. A
/// lone `.` is the exception: it names the archive root rather than
/// anything inside it, so there is no file for a key to address.
///
/// Joined by hand rather than with `PathBuf`, whose native separator
/// would desynchronize these keys from pnpm's always-forward-slashed
/// path layer and the `index.db` both implementations share. Callers
/// pass the `to_string_lossy` rendering, which coerces non-UTF-8 bytes
/// to U+FFFD per component.
pub(crate) fn clean_archive_entry_path(raw: &str) -> Result<String, TarballError> {
    let Some(mut parts) = archive_entry_segments(raw) else {
        return Err(TarballError::ReadTarballEntries(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "tar entry path rejected (non-normal component, possible directory traversal): {raw:?}",
            ),
        )));
    };
    if parts.as_slice() == ["."] {
        return Err(TarballError::ReadTarballEntries(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("tar entry path names the archive root itself, not a file in it: {raw:?}"),
        )));
    }
    if parts.len() > 1 {
        parts.remove(0);
    }
    Ok(parts.join("/"))
}

/// Reject a `package.json` entry that claims more than
/// [`MAX_UNTRUSTED_PREALLOC_BYTES`].
///
/// A manifest has to reach memory to be parsed — the bundled manifest
/// and its build-script detection both come from its bytes — so a
/// reader that otherwise holds only a bounded window has to draw the
/// line somewhere, and silently skipping the parse would record wrong
/// build metadata instead. Real manifests are a few KB; one past the
/// cap exists only in a hostile archive, so failing loudly is the
/// honest outcome.
pub(crate) fn oversized_manifest_error(file_size: u64) -> TarballError {
    TarballError::ReadTarballEntries(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!(
            "tar entry package.json is {file_size} bytes, which exceeds the \
             {MAX_UNTRUSTED_PREALLOC_BYTES}-byte manifest limit",
        ),
    ))
}

impl<'a> StreamingExtract<'a> {
    fn new(store_dir: &'a StoreDir) -> Self {
        StreamingExtract {
            store_dir,
            written: Vec::new(),
            batch: Vec::new(),
            batch_bytes: 0,
            manifest: None,
            build_hooks: false,
        }
    }

    /// A small entry is buffered and hashed in a batch; a large one is
    /// streamed straight into the CAFS. `package.json` is always buffered:
    /// its content is also the bundled manifest.
    fn add_entry(
        &mut self,
        entry: &mut tar::Entry<'_, impl Read>,
        meta: EntryMeta,
    ) -> Result<(), TarballError> {
        let is_manifest = meta.cleaned_path == "package.json";
        // A tar entry's payload can never exceed its header size, so
        // the pre-read check is sufficient.
        if is_manifest && meta.size > MAX_UNTRUSTED_PREALLOC_BYTES as u64 {
            return Err(oversized_manifest_error(meta.size));
        }
        if meta.size <= STREAM_ENTRY_BUFFER_MAX || is_manifest {
            return self.buffer_entry(entry, meta);
        }
        self.flush()?;
        self.stream_entry(entry, meta)
    }

    fn buffer_entry(
        &mut self,
        entry: &mut tar::Entry<'_, impl Read>,
        meta: EntryMeta,
    ) -> Result<(), TarballError> {
        let mut data = Vec::with_capacity(meta.size as usize);
        entry.read_to_end(&mut data).map_err(TarballError::ReadTarballEntries)?;
        if data.len() as u64 != meta.size {
            return Err(truncated_entry_error());
        }
        if meta.cleaned_path == "package.json" {
            let (build_scripts, manifest) = capture_bundled_manifest(&data);
            self.build_hooks |= build_scripts;
            self.manifest = manifest;
        }
        self.batch_bytes += data.len();
        self.batch.push(PendingFile {
            cleaned_path: meta.cleaned_path,
            data: Cow::Owned(data),
            executable: meta.executable,
            mode: meta.mode,
            size: meta.size,
        });
        if self.batch_bytes >= STREAM_BATCH_BUDGET_BYTES {
            self.flush()?;
        }
        Ok(())
    }

    fn stream_entry(
        &mut self,
        entry: &mut tar::Entry<'_, impl Read>,
        meta: EntryMeta,
    ) -> Result<(), TarballError> {
        // `Some(size)` makes the store writer reject a short stream
        // before anything is committed to a content-addressed path, so a
        // truncated archive leaves no orphan blob behind.
        let (file_path, file_hash, streamed_size) = self
            .store_dir
            .write_cas_file_from_reader(entry, meta.executable, Some(meta.size))
            .map_err(|error| match error {
                WriteCasFileFromReaderError::Read(error) => TarballError::ReadTarballEntries(error),
                WriteCasFileFromReaderError::Write(error) => TarballError::WriteCasFile(error),
            })?;
        self.written.push((
            meta.cleaned_path,
            file_path,
            cafs_file_info(&file_hash, meta.mode, streamed_size),
        ));
        Ok(())
    }

    fn flush(&mut self) -> Result<(), TarballError> {
        flush_pending_batch(
            self.store_dir,
            &mut self.batch,
            &mut self.batch_bytes,
            &mut self.written,
        )
    }

    fn finish(mut self) -> Result<(HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
        self.flush()?;
        Ok(assemble_extract_output(self.written, self.manifest, self.build_hooks))
    }
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
/// A leading `.` is preserved because npm's `tar` counts it as the
/// component removed by `strip: 1`. Other `.` components are ignored.
///
/// `None` for an absolute path or one climbing past the root.
pub(crate) fn archive_entry_segments(raw: &str) -> Option<Vec<&str>> {
    if raw.starts_with(['/', '\\']) {
        return None;
    }
    let mut segments = Vec::new();
    for (index, segment) in raw.split(['/', '\\']).enumerate() {
        match segment {
            "" => {}
            "." if index == 0 => segments.push(segment),
            "." => {}
            ".." => return None,
            other => segments.push(other),
        }
    }
    (!segments.is_empty()).then_some(segments)
}

mod streaming;

use streaming::{
    EntryMeta, STREAM_BATCH_BUDGET_BYTES, StreamingExtract, entry_meta, flush_pending_batch,
    truncated_entry_error,
};

mod manifest;
use manifest::capture_bundled_manifest;
