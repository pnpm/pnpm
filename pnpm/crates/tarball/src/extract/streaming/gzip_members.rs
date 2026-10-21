//! Hashing the tar a `.tgz` decodes to, one gzip member or all of them.

use super::{
    BodyHasher, Extraction, HashingReader, IgnoreEntryFilter, Integrity, Read, StoreDir, TarLimits,
    TarballError, drain_after_archive, extract_tarball_entries_streaming,
};

/// Extract the first gzip member of a `.tgz`, which a resource pin installs.
/// The member is the archive's tar only when nothing follows it.
pub(super) fn extract_first_member(
    gz_data: impl Read,
    tar_hasher: BodyHasher,
    limits: TarLimits,
    store_dir: &StoreDir,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
    package_url: &str,
) -> (Extraction, Result<Integrity, TarballError>) {
    let mut compressed = std::io::BufReader::new(gz_data);
    let mut tar = HashingReader {
        inner: flate2::bufread::GzDecoder::new(&mut compressed),
        hasher: tar_hasher,
        failure: None,
    };
    let extracted =
        extract_tarball_entries_streaming(&mut tar, limits, store_dir, ignore_file_pattern);
    let tar_integrity = finish_tar_of(&extracted, tar, limits, package_url)
        .and_then(|tar| match std::io::BufRead::fill_buf(&mut compressed) {
            Ok([]) => Ok(tar),
            _ => Err(TarballError::IncompleteTar {
                url: package_url.to_string(),
                reason: "data follows the first gzip member".to_string(),
            }),
        });
    (extracted, tar_integrity)
}

/// [`finish_tar`] for a tar the extractor accepted. A tar it rejected has no
/// integrity, so the rest of it is not read.
pub(super) fn finish_tar_of(
    extracted: &Extraction,
    tar: HashingReader<impl Read>,
    limits: TarLimits,
    package_url: &str,
) -> Result<Integrity, TarballError> {
    match extracted {
        Ok(_) => finish_tar(tar, limits.max_non_payload, package_url),
        Err(error) => Err(TarballError::IncompleteTar {
            url: package_url.to_string(),
            reason: error.to_string(),
        }),
    }
}

/// Read what the decoded tar has left into its hash. Any decode failure,
/// even one the extractor tolerated past the end-of-archive marker, leaves
/// the tar without an integrity.
fn finish_tar(
    mut tar: HashingReader<impl Read>,
    max_non_payload: u64,
    package_url: &str,
) -> Result<Integrity, TarballError> {
    let drained = drain_after_archive(&mut tar, max_non_payload);
    if let Some(reason) = tar.failure.take() {
        return Err(TarballError::IncompleteTar { url: package_url.to_string(), reason });
    }
    drained.map_err(|error| TarballError::IncompleteTar {
        url: package_url.to_string(),
        reason: error.to_string(),
    })?;
    tar.hasher.finish(package_url)
}

/// The streaming gzip extractor, also hashing the tar the archive
/// decodes to.
pub(crate) fn stream_extract_gzipped_tarball_with_tar(
    gz_data: &[u8],
    limits: TarLimits,
    store_dir: &StoreDir,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
) -> Result<crate::extract::GzipExtraction, TarballError> {
    let (extracted, tar) = extract_first_member(
        gz_data,
        BodyHasher::new(None),
        limits,
        store_dir,
        ignore_file_pattern,
        "",
    );
    let (files, index, bins) = extracted?;
    Ok((files, index, bins, tar))
}
