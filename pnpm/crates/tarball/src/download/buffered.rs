use super::{
    Arc, BodyHasher, ContentCoding, ExtractedResource, HashMap, IgnoreEntryFilter, Integrity,
    MAX_UNTRUSTED_PREALLOC_BYTES, PackageFilesIndex, PathBuf, StoreDir, StreamedResource,
    TarLimits, TarballError, extract_archive_bytes, extract_resource_stream,
    post_download_semaphore, spawn_extraction, verify_tarball_integrity,
};
use std::io::Read;

/// A whole archive response held in memory, as it arrived.
pub(crate) struct BufferedArchive<'a> {
    pub(crate) buffer: Vec<u8>,
    pub(crate) content_coding: ContentCoding,
    pub(crate) expected_integrity: Option<&'a Integrity>,
    pub(crate) unpacked_size: Option<usize>,
    pub(crate) tar_limits: TarLimits,
}

pub(crate) async fn extract_tarball_buffer(
    archive: BufferedArchive<'_>,
    package_url: &str,
    store_dir: &'static StoreDir,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
    let post_download_permit = post_download_semaphore()
        .acquire()
        .await
        .expect("post-download semaphore shouldn't be closed this soon");

    tracing::info!(target: "pacquet::download", ?package_url, "Download completed");

    let extraction = BufferedExtraction {
        buffer: archive.buffer,
        content_coding: archive.content_coding,
        expected_integrity: archive.expected_integrity.cloned(),
        unpacked_size: archive.unpacked_size,
        limits: archive.tar_limits.with_unpacked_size(archive.unpacked_size),
        package_url: package_url.to_string(),
        store_dir,
        ignore_file_pattern,
    };
    let result = spawn_extraction(post_download_permit, move || extraction.run())
        .await
        .map_err(TarballError::TaskJoin)??;

    tracing::info!(target: "pacquet::download", ?package_url, "Checksum verified");

    Ok(result)
}

/// The owned inputs of one buffered extraction, moved onto its blocking task.
struct BufferedExtraction {
    buffer: Vec<u8>,
    content_coding: ContentCoding,
    expected_integrity: Option<Integrity>,
    unpacked_size: Option<usize>,
    limits: TarLimits,
    package_url: String,
    store_dir: &'static StoreDir,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
}

impl BufferedExtraction {
    fn run(
        mut self,
    ) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
        let body = std::mem::take(&mut self.buffer);
        let resource = match decode_buffered_body(self.content_coding, body) {
            Ok(resource) => resource,
            Err(encoded) => return self.extract_encoded_stream(&encoded),
        };
        let integrity =
            verify_tarball_integrity(&resource, self.expected_integrity, self.package_url)?;
        let (cas_paths, pkg_files_idx) = extract_archive_bytes(
            &resource,
            self.unpacked_size,
            self.limits,
            self.store_dir,
            self.ignore_file_pattern.as_deref(),
        )?;
        Ok((integrity, cas_paths, pkg_files_idx))
    }

    fn extract_encoded_stream(
        &self,
        encoded: &[u8],
    ) -> Result<(Integrity, HashMap<String, PathBuf>, PackageFilesIndex), TarballError> {
        let (reader, watch) =
            self.content_coding.decode(encoded).map_err(TarballError::ReadTarballEntries)?;
        let resource = StreamedResource {
            hasher: BodyHasher::new(self.expected_integrity.as_ref()),
            package_url: &self.package_url,
            limits: self.limits,
            store_dir: self.store_dir,
            ignore_file_pattern: self.ignore_file_pattern.as_deref(),
        };
        let ExtractedResource { integrity, extracted } = extract_resource_stream(reader, resource);
        if let Some(error) = watch.error(&self.package_url) {
            return Err(error);
        }
        let integrity = integrity?;
        let (cas_paths, pkg_files_idx) = extracted?;
        Ok((integrity, cas_paths, pkg_files_idx))
    }
}

/// The resource a buffered body holds, with its content coding removed, as
/// long as it fits under [`MAX_UNTRUSTED_PREALLOC_BYTES`]. A larger one, or
/// one that does not decode, comes back as the encoded body to extract as a
/// stream, whose diagnostics then report it.
fn decode_buffered_body(content_coding: ContentCoding, body: Vec<u8>) -> Result<Vec<u8>, Vec<u8>> {
    if content_coding == ContentCoding::Identity {
        return Ok(body);
    }
    let limit = MAX_UNTRUSTED_PREALLOC_BYTES as u64;
    let mut decoded = Vec::new();
    let read = content_coding
        .decode(body.as_slice())
        .and_then(|(reader, _)| {
            reader
                .take(limit + 1)
                .read_to_end(&mut decoded)
        });
    match read {
        Ok(len) if (len as u64) <= limit => Ok(decoded),
        _ => Err(body),
    }
}
