//! Tarball decompression and entry extraction into the CAS.

pub(super) mod gzip;

pub(crate) use gzip::{
    GZIP_MAGIC, GzipExtraction, STREAM_EXTRACT_COMPRESSED_THRESHOLD,
    STREAM_EXTRACT_DURING_DOWNLOAD_THRESHOLD, decompress_gzip, extract_gzipped_tarball_with_tar,
    is_eager_decode_limit_exceeded,
};
pub use limits::{ArchiveLimit, DEFAULT_MAX_TARBALL_UNPACKED_SIZE, TarLimits};
pub(crate) use limits::{
    OversizedManifestError, RejectedEntryPathError, TarBudget, archive_entry_segments,
    clean_archive_entry_path, oversized_manifest_error,
};
pub use manifest::{TarInspection, inspect_tar, read_buffered_tar_entry, tar_requires_build};
pub(crate) use manifest::{
    apply_append_manifest, apply_placeholder_manifest, normalize_bundled_manifest,
};
pub(crate) use streaming::{
    BodyChunkReceiver, BodyChunkSender, ExtractedResource, STREAM_ENTRY_BUFFER_MAX,
    StreamedResource, body_chunk_channel, extract_resource_stream, stream_extract_channel,
    stream_extract_gzipped_tarball_with_tar, tar_entry_payload,
};

use std::collections::BTreeMap;

use super::{
    Cow, Cursor, HashMap, IgnoreEntryFilter, IntoParallelRefIterator, MAX_UNTRUSTED_PREALLOC_BYTES,
    ParallelIterator, PathBuf, Read, TarballError, UNIX_EPOCH, cas_write_pool,
};
use pnpm_fs::file_mode;
use pnpm_package_manifest::{
    BuildTriggers, PackageBin, ScriptRuntime, parse_manifest_bytes, parse_shebang_from_bytes,
    resolve_package_bins_with,
};
use pnpm_store_dir::{
    CafsFileInfo, FileHash, PackageFilesIndex, StoreDir, WriteCasFileFromReaderError,
};
use ssri::Integrity;
use tar::Archive;

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

/// Extract a fully buffered archive, a `.tgz` or a plain tar, into the CAFS.
/// A body that opens with the gzip magic is a `.tgz`; any other body is a tar.
///
/// For a `.tgz` it also returns the outcome of hashing the tar it decodes to.
pub(crate) fn extract_archive_bytes(
    resource: &[u8],
    unpacked_size: Option<usize>,
    limits: TarLimits,
    store_dir: &StoreDir,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
) -> Result<ExtractedArchiveBytes, TarballError> {
    if !resource.starts_with(&GZIP_MAGIC)
        && !resource.is_empty()
        && !is_tar_block(&resource[..resource.len().min(TAR_BLOCK)])
    {
        return Err(TarballError::UnrecognizedArchive);
    }
    if resource.starts_with(&GZIP_MAGIC) {
        let (files, index, bins, tar) = extract_gzipped_tarball_with_tar(
            resource,
            unpacked_size,
            limits,
            store_dir,
            ignore_file_pattern,
        )?;
        return Ok(ExtractedArchiveBytes { files, index, bins, tar: Some(tar) });
    }
    let (files, index, bins) = extract_tarball_entries_with_bins(
        resource,
        limits.for_tar(),
        store_dir,
        ignore_file_pattern,
    )?;
    Ok(ExtractedArchiveBytes { files, index, bins, tar: None })
}

/// The output of [`extract_archive_bytes`]. `tar` is `None` for a tar
/// resource, which is its own tar.
pub(crate) struct ExtractedArchiveBytes {
    pub(crate) files: HashMap<String, PathBuf>,
    pub(crate) index: PackageFilesIndex,
    pub(crate) bins: Option<BTreeMap<String, PackageBin>>,
    pub(crate) tar: Option<Result<Integrity, TarballError>>,
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
    let checked_at = UNIX_EPOCH
        .elapsed()
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok());
    CafsFileInfo { digest: format!("{file_hash:x}"), mode, size, checked_at }
}

pub(crate) type ExtractedEntries =
    (HashMap<String, PathBuf>, PackageFilesIndex, Option<BTreeMap<String, PackageBin>>);

/// Walk decompressed tar bytes, writing each regular-file entry into
/// the CAFS and returning the `{in-tarball path → CAFS path}` map plus
/// the per-tarball [`PackageFilesIndex`] row to hand off to the shared
/// store-index writer.
///
/// Only regular files are stored; a published npm tarball carries
/// nothing else that pacquet can represent.
///
/// Every tar-side failure comes back as
/// [`TarballError::ReadTarballEntries`] instead of panicking, and a
/// non-UTF-8 entry path is coerced via
/// [`std::path::Path::to_string_lossy`] to match pnpm's string-based
/// handling, so a mixed install against the shared `index.db` agrees.
pub(crate) fn extract_tarball_entries_with_bins(
    tar_data: &[u8],
    limits: TarLimits,
    store_dir: &StoreDir,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
) -> Result<ExtractedEntries, TarballError> {
    let (pending, manifest, requires_build) =
        collect_pending_tar_entries(tar_data, limits, ignore_file_pattern)?;
    let bins = resolve_pending_bins(manifest.as_ref(), &pending);
    let written = write_pending_files(store_dir, &pending)?;
    let (files, index) = assemble_extract_output(written, manifest, requires_build);
    Ok((files, index, bins))
}

fn collect_pending_tar_entries<'a>(
    tar_data: &'a [u8],
    limits: TarLimits,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
) -> Result<(Vec<PendingFile<'a>>, Option<serde_json::Value>, bool), TarballError> {
    let budget = TarBudget::new(limits);
    let mut archive = Archive::new(Cursor::new(tar_data));
    archive.set_max_metadata_size(Some(crate::MAX_TARBALL_METADATA_BYTES));
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
    let mut pending: Vec<PendingFile<'_>> = Vec::with_capacity(capacity);
    let mut manifest = None;
    let mut triggers = BuildTriggers::default();

    for entry in entries {
        let entry = entry.map_err(TarballError::ReadTarballEntries)?;
        budget.announce_file(entry.size())?;
        let Some(meta) = entry_meta(&entry, ignore_file_pattern)? else {
            continue;
        };
        let entry_data = tar_entry_payload(tar_data, &entry)?;

        triggers.add_file(&meta.cleaned_path);
        if meta.cleaned_path == "package.json" {
            manifest = capture_bundled_manifest(entry_data, &mut triggers);
        }

        pending.push(PendingFile {
            cleaned_path: meta.cleaned_path,
            data: Cow::Borrowed(entry_data),
            executable: meta.executable,
            mode: meta.mode,
            size: meta.size,
        });
    }
    budget.check_buffered_end(tar_data, archive.into_inner().position())?;
    Ok((pending, manifest, triggers.requires_build()))
}

fn resolve_pending_bins(
    manifest: Option<&serde_json::Value>,
    pending: &[PendingFile<'_>],
) -> Option<BTreeMap<String, PackageBin>> {
    let parsed = manifest?;
    resolve_package_bins_with(
        parsed,
        |path| {
            pending
                .iter()
                .any(|file| file.cleaned_path == path)
        },
        pending.iter().map(|file| file.cleaned_path.as_str()),
        |path| {
            let file = pending
                .iter()
                .rfind(|file| file.cleaned_path == path)?;
            let head = &file.data[..file.data.len().min(512)];
            parse_shebang_from_bytes(head)
        },
    )
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
            pending
                .par_iter()
                .map(|file| write_cas_entry(store_dir, file))
                .collect()
        };
        match cas_write_pool() {
            Some(pool) => pool.install(write_all),
            None => write_all(),
        }
    } else {
        pending
            .iter()
            .map(|file| write_cas_entry(store_dir, file))
            .collect()
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
        tar_integrity: None,
    };
    (cas_paths, pkg_files_idx)
}

impl<'a> StreamingExtract<'a> {
    fn new(store_dir: &'a StoreDir) -> Self {
        StreamingExtract {
            store_dir,
            written: Vec::new(),
            batch: Vec::new(),
            batch_bytes: 0,
            manifest: None,
            triggers: BuildTriggers::default(),
            shebangs: HashMap::new(),
        }
    }

    fn record_shebang(&mut self, cleaned_path: &str, head: &[u8]) {
        if let Some(runtime) = parse_shebang_from_bytes(head) {
            self.shebangs.insert(cleaned_path.to_string(), runtime);
        } else if !self.shebangs.is_empty() {
            self.shebangs.remove(cleaned_path);
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
            self.manifest = capture_bundled_manifest(&data, &mut self.triggers);
        } else {
            let head = &data[..data.len().min(512)];
            self.record_shebang(&meta.cleaned_path, head);
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
        let mut head = [0u8; 512];
        let head_len = streaming::read_prefix(entry, &mut head)?;
        self.record_shebang(&meta.cleaned_path, &head[..head_len]);
        // `Some(size)` makes the store writer reject a short stream
        // before anything is committed to a content-addressed path, so a
        // truncated archive leaves no orphan blob behind.
        let (file_path, file_hash, streamed_size) = self.store_dir
            .write_cas_file_from_reader(
                &mut (&head[..head_len]).chain(entry),
                meta.executable,
                Some(meta.size),
            )
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

    fn finish(mut self) -> Result<ExtractedEntries, TarballError> {
        self.flush()?;
        let (files, index) =
            assemble_extract_output(self.written, self.manifest, self.triggers.requires_build());
        let bins = index.manifest
            .as_ref()
            .and_then(|parsed| {
                resolve_package_bins_with(
                    parsed,
                    |path| files.contains_key(path),
                    files.keys().map(String::as_str),
                    |path| self.shebangs.get(path).cloned(),
                )
            });
        Ok((files, index, bins))
    }
}

mod limits;
use limits::{TAR_BLOCK, check_trailer, is_tar_block};

mod streaming;

use streaming::{
    EntryMeta, STREAM_BATCH_BUDGET_BYTES, StreamingExtract, entry_meta, flush_pending_batch,
    truncated_entry_error,
};

mod manifest;
use manifest::capture_bundled_manifest;
