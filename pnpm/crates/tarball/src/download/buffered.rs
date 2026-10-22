use super::{
    Arc, ContentCoding, ExtractedResource, HashMap, IgnoreEntryFilter,
    MAX_UNTRUSTED_PREALLOC_BYTES, PackageFilesIndex, PathBuf, StoreDir, StreamedResource,
    TarLimits, TarballError, extract_archive_bytes, extract_resource_stream,
    post_download_semaphore, spawn_extraction, verify_tarball_integrity,
};
use crate::{
    ArchiveIntegrity,
    archive_integrity::ArchiveVerification,
    extract::{ExtractedArchiveBytes, GZIP_MAGIC},
};
use pnpm_package_manifest::PackageBin;
use std::{collections::BTreeMap, io::Read};

/// A whole archive response held in memory, as it arrived.
pub(crate) struct BufferedArchive {
    pub(crate) buffer: Vec<u8>,
    pub(crate) content_coding: ContentCoding,
    pub(crate) verification: ArchiveVerification,
    pub(crate) unpacked_size: Option<usize>,
    pub(crate) tar_limits: TarLimits,
}

/// The integrities, CAFS files, store row, and precalculated bins of one extracted archive.
pub(crate) type ExtractedFiles = (
    ArchiveIntegrity,
    HashMap<String, PathBuf>,
    PackageFilesIndex,
    Option<BTreeMap<String, PackageBin>>,
);

pub(crate) async fn extract_tarball_buffer(
    archive: BufferedArchive,
    package_url: &str,
    store_dir: &'static StoreDir,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
) -> Result<ExtractedFiles, TarballError> {
    let post_download_permit = post_download_semaphore()
        .acquire()
        .await
        .expect("post-download semaphore shouldn't be closed this soon");

    tracing::info!(target: "pacquet::download", ?package_url, "Download completed");

    let extraction = BufferedExtraction {
        buffer: archive.buffer,
        content_coding: archive.content_coding,
        verification: archive.verification,
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
    verification: ArchiveVerification,
    unpacked_size: Option<usize>,
    limits: TarLimits,
    package_url: String,
    store_dir: &'static StoreDir,
    ignore_file_pattern: Option<Arc<IgnoreEntryFilter>>,
}

impl BufferedExtraction {
    fn run(mut self) -> Result<ExtractedFiles, TarballError> {
        let body = std::mem::take(&mut self.buffer);
        let resource = match decode_buffered_body(self.content_coding, body, self.unpacked_size) {
            Ok(resource) => resource,
            Err(encoded) => return self.extract_stream(self.content_coding, &encoded),
        };
        // A pin that may name the tar is checked against every gzip member,
        // which only the streaming decoder reads.
        if self.verification.needs_tar_stream() && resource.starts_with(&GZIP_MAGIC) {
            return self.extract_stream(ContentCoding::Identity, &resource);
        }
        // Here the pin, if any, names the resource: checking it first keeps a
        // tampered archive out of the store.
        let integrity = verify_tarball_integrity(
            &resource,
            self.verification.expected.clone(),
            self.package_url.clone(),
        )?;
        let ExtractedArchiveBytes { files, index, bins, tar } = extract_archive_bytes(
            &resource,
            self.unpacked_size,
            self.limits,
            self.store_dir,
            self.ignore_file_pattern.as_deref(),
        )?;
        let tar = tar.map(|tar| {
            tar.map_err(|error| match error {
                TarballError::IncompleteTar { reason, .. } => {
                    TarballError::IncompleteTar { url: self.package_url.clone(), reason }
                }
                error => error,
            })
        });
        let integrity = self.verification.finish(Ok(integrity), tar, &self.package_url)?;
        Ok((integrity, files, index, bins))
    }

    fn extract_stream(
        &self,
        content_coding: ContentCoding,
        encoded: &[u8],
    ) -> Result<ExtractedFiles, TarballError> {
        let (reader, watch) =
            content_coding.decode(encoded).map_err(TarballError::ReadTarballEntries)?;
        let resource = StreamedResource {
            verification: self.verification.clone(),
            package_url: &self.package_url,
            limits: self.limits,
            store_dir: self.store_dir,
            ignore_file_pattern: self.ignore_file_pattern.as_deref(),
        };
        let ExtractedResource { integrity, extracted } = extract_resource_stream(reader, &resource);
        if let Some(error) = watch.error(&self.package_url) {
            return Err(error);
        }
        let integrity = integrity?;
        let (cas_paths, pkg_files_idx, bins) = extracted?;
        Ok((integrity, cas_paths, pkg_files_idx, bins))
    }
}

/// The resource a buffered body holds, with its content coding removed, as
/// long as it fits under [`MAX_UNTRUSTED_PREALLOC_BYTES`]. A larger one, or
/// one that does not decode, comes back as the encoded body to extract as a
/// stream, whose diagnostics then report it. A body whose `unpacked_size` or
/// zstd frame header already says it is larger is not decoded here at all.
fn decode_buffered_body(
    content_coding: ContentCoding,
    body: Vec<u8>,
    unpacked_size: Option<usize>,
) -> Result<Vec<u8>, Vec<u8>> {
    if content_coding == ContentCoding::Identity {
        return Ok(body);
    }
    let limit = MAX_UNTRUSTED_PREALLOC_BYTES as u64;
    if [unpacked_size.map(|size| size as u64), content_coding.declared_size(&body)]
        .into_iter()
        .flatten()
        .any(|size| size > limit)
    {
        return Err(body);
    }
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

#[cfg(test)]
mod tests;
