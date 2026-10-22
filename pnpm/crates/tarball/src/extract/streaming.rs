pub(crate) use gzip_members::stream_extract_gzipped_tarball_with_tar;

use super::{
    Archive, BTreeMap, BuildTriggers, CafsFileInfo, ExtractedEntries, GZIP_MAGIC, HashMap,
    IgnoreEntryFilter, PackageBin, PackageFilesIndex, PathBuf, PendingFile, Read, ScriptRuntime,
    StoreDir, TAR_BLOCK, TarBudget, TarLimits, TarballError, check_trailer,
    clean_archive_entry_path, file_mode, is_tar_block, write_pending_files,
};
use crate::{
    ArchiveIntegrity, BodyHasher, IntegrityKind,
    archive_integrity::{ArchiveHashers, ArchiveVerification},
    content_coding::ContentCoding,
};
use ssri::Integrity;

/// Body chunks in flight between the download loop and the extractor.
///
/// The queue is what keeps the two decoupled — the extractor is
/// normally far faster than the network, and a few chunks of slack stop
/// a momentary stall on either side from costing throughput. It is
/// bounded because the alternative is a queue that grows to whatever a
/// server sends faster than the extractor can consume it, which would
/// hand back the unbounded buffer this path exists to avoid. Reaching
/// the bound applies backpressure to the download rather than failing
/// it.
pub(crate) const STREAM_CHANNEL_CHUNKS: usize = 64;

/// Sender half of the queue [`ChannelBytesReader`] drains.
///
/// `Ok` items are payload. An `Err` item is the download loop reporting
/// that the body failed mid-stream; it surfaces as the reader's error
/// so the extractor unwinds instead of mistaking a truncated body for a
/// complete archive.
pub(crate) type BodyChunkSender = tokio::sync::mpsc::Sender<std::io::Result<bytes::Bytes>>;

/// Receiver half of [`BodyChunkSender`].
pub(crate) type BodyChunkReceiver = tokio::sync::mpsc::Receiver<std::io::Result<bytes::Bytes>>;

/// Allocate the bounded queue joining an async download loop to a
/// blocking extractor.
pub(crate) fn body_chunk_channel() -> (BodyChunkSender, BodyChunkReceiver) {
    tokio::sync::mpsc::channel(STREAM_CHANNEL_CHUNKS)
}

/// Blocking [`Read`] over a channel of downloaded body chunks: the
/// bridge that lets [`extract_tarball_entries_streaming`] run on a
/// blocking thread while the async download loop keeps feeding it.
///
/// A closed channel (sender dropped after the last chunk) is
/// end-of-stream.
pub(crate) struct ChannelBytesReader {
    pub(super) rx: BodyChunkReceiver,
    pub(super) current: bytes::Bytes,
    pub(super) offset: usize,
}

impl ChannelBytesReader {
    pub(crate) fn new(rx: BodyChunkReceiver) -> Self {
        Self { rx, current: bytes::Bytes::new(), offset: 0 }
    }
}

impl Read for ChannelBytesReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        while self.offset >= self.current.len() {
            // Only ever reached from the blocking thread the extractor
            // runs on, which is where `blocking_recv` belongs; it
            // panics inside an async context.
            match self.rx.blocking_recv() {
                Some(Ok(chunk)) => {
                    self.current = chunk;
                    self.offset = 0;
                }
                Some(Err(error)) => return Err(error),
                None => return Ok(0),
            }
        }
        let take = (self.current.len() - self.offset).min(buf.len());
        buf[..take].copy_from_slice(&self.current[self.offset..self.offset + take]);
        self.offset += take;
        Ok(take)
    }
}

/// Decodes, hashes, and CAS-writes a download delivered in chunks through
/// `rx`. This runs on a blocking thread for the lifetime of the download.
pub(crate) fn stream_extract_channel(
    rx: BodyChunkReceiver,
    content_coding: ContentCoding,
    resource: &StreamedResource<'_>,
) -> ExtractedResource {
    let package_url = resource.package_url;
    match content_coding.decode(ChannelBytesReader::new(rx)) {
        Ok((reader, watch)) => {
            let extracted = extract_resource_stream(reader, resource);
            match watch.error(package_url) {
                Some(error) => ExtractedResource::failed(error),
                None => extracted,
            }
        }
        Err(error) => ExtractedResource::failed(TarballError::ReadTarballEntries(error)),
    }
}

/// What one streamed extraction checks the resource against and writes it to.
pub(crate) struct StreamedResource<'a> {
    pub(crate) verification: ArchiveVerification,
    pub(crate) package_url: &'a str,
    pub(crate) limits: TarLimits,
    pub(crate) store_dir: &'a StoreDir,
    pub(crate) ignore_file_pattern: Option<&'a IgnoreEntryFilter>,
}

/// The two verdicts on a streamed resource. They are kept apart so the caller
/// can rank an integrity mismatch above whatever it made the extractor do.
pub(crate) struct ExtractedResource {
    pub(crate) integrity: Result<ArchiveIntegrity, TarballError>,
    pub(crate) extracted: Extraction,
}

impl ExtractedResource {
    /// A resource that failed before either verdict could be reached.
    pub(crate) fn failed(error: TarballError) -> Self {
        Self {
            integrity: Err(error),
            extracted: Err(TarballError::ReadTarballEntries(std::io::Error::other(
                "the resource was not read",
            ))),
        }
    }
}

/// Hash `reader`, the resource with any content coding removed, and extract
/// it as a `.tgz` when it opens with the gzip magic and as a tar otherwise.
///
/// The hash covers the whole resource, so what is left after the archive
/// ends is read too, up to the non-payload limit. A `.tgz` is hashed a second
/// time as the tar it decodes to, every gzip member of it.
pub(crate) fn extract_resource_stream(
    mut reader: impl Read,
    resource: &StreamedResource<'_>,
) -> ExtractedResource {
    let mut head = [0u8; TAR_BLOCK];
    let head_len = match read_prefix(&mut reader, &mut head) {
        Ok(head_len) => head_len,
        Err(error) => return ExtractedResource::failed(error),
    };
    let prefix = &head[..head_len];
    let gzip = prefix.starts_with(&GZIP_MAGIC);
    let ArchiveHashers { resource: mut resource_hasher, tar } = resource
        .verification
        .hashers(gzip);
    resource_hasher.input(prefix);
    let mut hashing = HashingReader { inner: reader, hasher: resource_hasher, failure: None };
    let (extracted, tar) = if !gzip && !prefix.is_empty() && !is_tar_block(prefix) {
        (Err(TarballError::UnrecognizedArchive), None)
    } else {
        resource.extract(prefix.chain(&mut hashing), tar)
    };
    finish_resource(hashing, extracted, tar, resource)
}

impl StreamedResource<'_> {
    /// Extract `body`, a tar when `tar_hasher` is `None` and a `.tgz`
    /// otherwise, and hash the tar a `.tgz` decodes to.
    fn extract(
        &self,
        body: impl Read,
        tar_hasher: Option<BodyHasher>,
    ) -> (Extraction, Option<Result<Integrity, TarballError>>) {
        let Some(tar_hasher) = tar_hasher else {
            let extracted = extract_tarball_entries_streaming(
                body,
                self.limits.for_tar(),
                self.store_dir,
                self.ignore_file_pattern,
            );
            return (extracted, None);
        };
        if self.verification.kind == IntegrityKind::Resource {
            let (extracted, tar) = extract_first_member(
                body,
                tar_hasher,
                self.limits,
                self.store_dir,
                self.ignore_file_pattern,
                self.package_url,
            );
            return (extracted, Some(tar));
        }
        let mut tar = HashingReader {
            inner: flate2::read::MultiGzDecoder::new(body),
            hasher: tar_hasher,
            failure: None,
        };
        let extracted = extract_tarball_entries_streaming(
            &mut tar,
            self.limits,
            self.store_dir,
            self.ignore_file_pattern,
        );
        let tar = finish_tar_of(&extracted, tar, self.limits, self.package_url);
        (extracted, Some(tar))
    }
}

/// The CAFS files, the store row, and precalculated bins of one extraction.
pub(crate) type Extraction = Result<
    (HashMap<String, PathBuf>, PackageFilesIndex, Option<BTreeMap<String, PackageBin>>),
    TarballError,
>;

/// Drain what follows the archive into its hash and pair the hashes with
/// the extraction's verdict. A failed drain leaves no hash, and an extraction
/// error then says why.
fn finish_resource(
    mut hashing: HashingReader<impl Read>,
    extracted: Extraction,
    tar: Option<Result<Integrity, TarballError>>,
    resource: &StreamedResource<'_>,
) -> ExtractedResource {
    let integrity = match drain_after_archive(&mut hashing, resource.limits.max_non_payload) {
        Ok(()) => hashing.hasher.finish(resource.package_url),
        Err(drain) => match extracted {
            Err(error) => return ExtractedResource::failed(error),
            Ok(_) => Err(drain),
        },
    };
    let integrity = match resource.verification.finish(integrity, tar, resource.package_url) {
        Err(TarballError::IncompleteTar { .. }) if extracted.is_err() => {
            return ExtractedResource::failed(extracted.expect_err(
                "guarded by extracted.is_err()",
            ));
        }
        other => other,
    };
    ExtractedResource { integrity, extracted }
}

pub(super) fn read_prefix(
    reader: &mut impl Read,
    prefix: &mut [u8],
) -> Result<usize, TarballError> {
    let mut filled = 0;
    while filled < prefix.len() {
        match reader.read(&mut prefix[filled..]) {
            Ok(0) => break,
            Ok(len) => filled += len,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(TarballError::ReadTarballEntries(error)),
        }
    }
    Ok(filled)
}

/// Read the rest of the resource into its hash, failing past `limit`.
fn drain_after_archive(reader: &mut impl Read, limit: u64) -> Result<(), TarballError> {
    let drained = std::io::copy(&mut reader.take(limit + 1), &mut std::io::sink())
        .map_err(TarballError::ReadTarballEntries)?;
    if drained > limit {
        return Err(TarballError::ArchiveLimit(super::ArchiveLimit::NonPayload(limit)));
    }
    Ok(())
}

/// Feeds every byte read through it into a [`BodyHasher`], and remembers
/// the first read error, which a reader above it may swallow.
struct HashingReader<Inner> {
    inner: Inner,
    hasher: BodyHasher,
    failure: Option<String>,
}

impl<Inner: Read> Read for HashingReader<Inner> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = self.inner
            .read(buf)
            .inspect_err(|error| {
                self.failure.get_or_insert_with(|| error.to_string());
            })?;
        self.hasher.input(&buf[..len]);
        Ok(len)
    }
}

/// Ceiling for buffering one tar entry in memory on the streaming
/// extraction path. Entries at or below this go through the batched
/// [`write_pending_files`] fan-out; larger ones stream straight into
/// the store via [`StoreDir::write_cas_file_from_reader`] without ever
/// being held in memory. `package.json` is the exception — the bundled
/// manifest must be parsed from bytes (build-script detection depends
/// on it), so it is buffered up to [`MAX_UNTRUSTED_PREALLOC_BYTES`](crate::MAX_UNTRUSTED_PREALLOC_BYTES),
/// beyond which the archive is rejected as hostile.
pub(crate) const STREAM_ENTRY_BUFFER_MAX: u64 = 4 * 1024 * 1024;

/// Byte budget for one batch of buffered entries on the streaming
/// extraction path. A batch flushes to [`write_pending_files`] once it
/// holds this much payload, so peak memory stays bounded by the budget
/// (plus one in-flight entry) instead of the archive's unpacked size,
/// while typical batches are still large enough for the parallel
/// CAS-write fan-out to pay off.
pub(super) const STREAM_BATCH_BUDGET_BYTES: usize = 32 * 1024 * 1024;

/// Walk a tar stream, writing each regular-file entry into the CAFS
/// and returning the same outputs as [`extract_tarball_entries_with_bins`](crate::extract::extract_tarball_entries_with_bins),
/// while holding only a bounded window of the archive in memory.
///
/// Small entries are buffered and flushed in bounded batches through
/// the same parallel write phase as the eager path; entries above
/// [`STREAM_ENTRY_BUFFER_MAX`] stream straight into the store with an
/// incremental hash. A batch always flushes before a streamed entry is
/// written, so the output rows keep archive order and the last-wins
/// duplicate semantics of [`assemble_extract_output`](crate::extract::assemble_extract_output) hold.
///
/// Decoder failures (e.g. a corrupt gzip stream) surface through the
/// reader as [`TarballError::ReadTarballEntries`]; the retry
/// classifier treats them the same as an eager-path decode error.
pub(crate) fn extract_tarball_entries_streaming(
    reader: impl Read,
    limits: TarLimits,
    store_dir: &StoreDir,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
) -> Result<ExtractedEntries, TarballError> {
    let budget = TarBudget::new(limits);
    walk_tar_stream(budget.reader(reader), &budget, store_dir, ignore_file_pattern)
        .map_err(|error| budget.explain(error))
}

fn walk_tar_stream(
    reader: impl Read,
    budget: &TarBudget,
    store_dir: &StoreDir,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
) -> Result<ExtractedEntries, TarballError> {
    let mut archive = Archive::new(reader);
    archive.set_max_metadata_size(Some(crate::MAX_TARBALL_METADATA_BYTES));
    let mut extract = StreamingExtract::new(store_dir);

    for entry in archive.entries().map_err(TarballError::ReadTarballEntries)? {
        let mut entry = entry.map_err(TarballError::ReadTarballEntries)?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        budget.announce_file(entry.size())?;
        let Some(meta) = entry_meta(&entry, ignore_file_pattern)? else {
            continue;
        };
        extract.triggers.add_file(&meta.cleaned_path);
        extract.add_entry(&mut entry, meta)?;
    }
    check_trailer(archive.into_inner())?;
    budget.ensure_within()?;
    budget.check_not_empty()?;
    extract.finish()
}

/// The header fields of one regular-file tar entry, with its path validated
/// and cleaned.
pub(super) struct EntryMeta {
    pub(super) cleaned_path: String,
    pub(super) executable: bool,
    pub(super) mode: u32,
    pub(super) size: u64,
}

/// `None` when the ignore filter drops the entry.
/// Bypassing the CAS write here also keeps the package's
/// [`PackageFilesIndex`] tight — an ignored entry never surfaces in `files`
/// or `manifest`.
pub(super) fn entry_meta<Source: Read>(
    entry: &tar::Entry<'_, Source>,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
) -> Result<Option<EntryMeta>, TarballError> {
    let mode = entry
        .header()
        .mode()
        .map_err(TarballError::ReadTarballEntries)?;
    let size = entry
        .header()
        .size()
        .map_err(TarballError::ReadTarballEntries)?;
    let cleaned_path = {
        let entry_path = entry.path().map_err(TarballError::ReadTarballEntries)?;
        clean_archive_entry_path(&entry_path.to_string_lossy())?
    };
    if ignore_file_pattern.is_some_and(|filter| filter(&cleaned_path)) {
        return Ok(None);
    }
    Ok(Some(EntryMeta { cleaned_path, executable: file_mode::is_executable(mode), mode, size }))
}

/// The running state of a streaming extraction: the CAFS rows written so
/// far, the batch of small entries waiting to be written, and what the
/// archive said about build hooks.
pub(super) struct StreamingExtract<'a> {
    pub(super) store_dir: &'a StoreDir,
    pub(super) written: Vec<(String, PathBuf, CafsFileInfo)>,
    pub(super) batch: Vec<PendingFile<'static>>,
    pub(super) batch_bytes: usize,
    pub(super) manifest: Option<serde_json::Value>,
    pub(super) triggers: BuildTriggers,
    pub(super) shebangs: HashMap<String, ScriptRuntime>,
}

pub(super) fn truncated_entry_error() -> TarballError {
    TarballError::ReadTarballEntries(std::io::Error::new(
        std::io::ErrorKind::UnexpectedEof,
        "tar entry payload extends beyond archive",
    ))
}

/// Hash and write the buffered batch into the CAFS, appending its rows
/// to `written` in order and resetting the batch accumulator.
pub(super) fn flush_pending_batch(
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
    let file_size = entry
        .header()
        .size()
        .map_err(TarballError::ReadTarballEntries)?;
    let data_offset = usize::try_from(entry.raw_file_position())
        .map_err(|_| invalid("tar entry file offset does not fit in usize"))?;
    let size = usize::try_from(file_size)
        .map_err(|_| invalid("tar entry file size does not fit in usize"))?;
    let end = data_offset
        .checked_add(size)
        .ok_or_else(|| invalid("tar entry file offset plus size overflows usize"))?;
    tar_data
        .get(data_offset..end)
        .ok_or_else(|| {
            TarballError::ReadTarballEntries(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "tar entry payload extends beyond archive",
            ))
        })
}

mod gzip_members;

use gzip_members::{extract_first_member, finish_tar_of};
