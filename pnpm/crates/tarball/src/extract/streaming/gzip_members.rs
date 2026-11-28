//! Hashing the tar a `.tgz` decodes to, one gzip member or all of them.

use std::io::BufRead;

use super::{
    BodyHasher,
    Extraction,
    HashingReader,
    IgnoreEntryFilter,
    Integrity,
    Read,
    StoreDir,
    TarLimits,
    TarballError,
    drain_after_archive,
    extract_tarball_entries_streaming,
};

/// Extract a `.tgz` for a resource pin, which installs its first gzip
/// member. The member is the archive's tar only when nothing follows it.
///
/// A tar that runs past the end of the first member goes on into the
/// members after it, as node-tar reads it.
pub(super) fn extract_first_member(
    gz_data: impl Read,
    tar_hasher: BodyHasher,
    limits: TarLimits,
    store_dir: &StoreDir,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
    package_url: &str,
) -> (Extraction, Result<Integrity, TarballError>) {
    let mut members = GzipMembers::new(std::io::BufReader::new(gz_data));
    let mut tar = HashingReader { inner: &mut members, hasher: tar_hasher, failure: None };
    let extracted =
        extract_tarball_entries_streaming(&mut tar, limits, store_dir, ignore_file_pattern);
    tar.inner.stay_in_current_member();
    let tar_integrity = finish_tar_of(&extracted, tar, limits, package_url)
        .and_then(|tar| match members.data_follows_first() {
            Ok(false) => Ok(tar),
            _ => Err(TarballError::IncompleteTar {
                url: package_url.to_string(),
                reason: "data follows the first gzip member".to_string(),
            }),
        });
    (extracted, tar_integrity)
}

/// Decodes the first gzip member of a `.tgz`, and the members after it when
/// a read reaches the end of the first.
struct GzipMembers<Compressed: BufRead> {
    decoder: Option<GzipMember<Compressed>>,
    may_cross: bool,
}

enum GzipMember<Compressed: BufRead> {
    First(flate2::bufread::GzDecoder<Compressed>),
    Rest(flate2::bufread::MultiGzDecoder<Compressed>),
}

impl<Compressed: BufRead> GzipMembers<Compressed> {
    fn new(compressed: Compressed) -> Self {
        let first = GzipMember::First(flate2::bufread::GzDecoder::new(compressed));
        Self { decoder: Some(first), may_cross: true }
    }

    /// End the decoded stream at the end of the member being read.
    fn stay_in_current_member(&mut self) {
        self.may_cross = false;
    }

    /// Whether data follows the first member: a read went past it, or the
    /// compressed input goes on after it.
    fn data_follows_first(&mut self) -> std::io::Result<bool> {
        match &mut self.decoder {
            Some(GzipMember::First(first)) => Ok(!first.get_mut().fill_buf()?.is_empty()),
            _ => Ok(true),
        }
    }
}

impl<Compressed: BufRead> Read for GzipMembers<Compressed> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            match &mut self.decoder {
                Some(GzipMember::First(first)) => {
                    let read = first.read(buf)?;
                    if read != 0
                        || buf.is_empty()
                        || !self.may_cross
                        || first.get_mut().fill_buf()?.is_empty()
                    {
                        return Ok(read);
                    }
                }
                Some(GzipMember::Rest(rest)) => return rest.read(buf),
                None => unreachable!("the decoder is only taken to be replaced"),
            }
            if let Some(GzipMember::First(first)) = self.decoder.take() {
                let rest = flate2::bufread::MultiGzDecoder::new(first.into_inner());
                self.decoder = Some(GzipMember::Rest(rest));
            }
        }
    }
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
