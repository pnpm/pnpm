use super::{
    HashMap, IgnoreEntryFilter, PackageFilesIndex, PathBuf, StoreDir, TarLimits, TarballError,
    decompress_gzip, extract_tarball_entries, gzip_isize_hint, is_eager_decode_limit_exceeded,
    should_stream_extract, stream_extract_gzipped_tarball_with_tar,
};
use ssri::Integrity;

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
/// No archive is refused for its size beyond `limits`. Both outcomes route to
/// [`stream_extract_gzipped_tarball_with_tar`], which decodes the same bytes
/// with the same results in bounded memory.
///
/// The outcome of hashing the tar the archive decodes to comes with the
/// files.
pub(crate) fn extract_gzipped_tarball_with_tar(
    gz_data: &[u8],
    unpacked_size: Option<usize>,
    limits: TarLimits,
    store_dir: &StoreDir,
    ignore_file_pattern: Option<&IgnoreEntryFilter>,
) -> Result<GzipExtraction, TarballError> {
    // Route on the larger of the two claims about the unpacked size.
    // Neither is trustworthy, and taking the larger is the conservative
    // reading: a registry hint that under-reports cannot hide a trailer
    // that does not, or the other way round.
    let unpacked_size = unpacked_size.max(gzip_isize_hint(gz_data));
    if should_stream_extract(gz_data.len(), unpacked_size) {
        return stream_extract_gzipped_tarball_with_tar(
            gz_data,
            limits,
            store_dir,
            ignore_file_pattern,
        );
    }
    match decompress_gzip(gz_data, unpacked_size) {
        Ok(tar_data) => {
            let (files, index) =
                extract_tarball_entries(&tar_data, limits, store_dir, ignore_file_pattern)?;
            Ok((files, index, eager_tar_integrity(gz_data, &tar_data)))
        }
        Err(error) if is_eager_decode_limit_exceeded(&error) => {
            tracing::debug!(
                target: "pacquet::download",
                gz_data_len = gz_data.len(),
                "archive inflated past the eager decode ceiling; extracting it as a stream",
            );
            stream_extract_gzipped_tarball_with_tar(gz_data, limits, store_dir, ignore_file_pattern)
        }
        Err(error) => Err(error),
    }
}

/// The CAFS files, the store row, and the outcome of hashing the decoded tar.
pub(crate) type GzipExtraction =
    (HashMap<String, PathBuf>, PackageFilesIndex, Result<Integrity, TarballError>);

/// The integrity of `tar_data`, the eager decoder's output for `gz_data`,
/// when it is the whole archive. The eager decoder reads one gzip member and
/// skips its checksum, so the member's trailer, which is the archive's last 8
/// bytes only if the member is the only one, must describe exactly `tar_data`.
///
/// The decoder does not report where the member ends, so a publisher can
/// append data whose last 8 bytes repeat the trailer. The tar recorded for
/// such an archive is then its first member, which a tar pin checked by the
/// streaming decoder rejects: the mistake fails installs, it never changes
/// what they install.
fn eager_tar_integrity(gz_data: &[u8], tar_data: &[u8]) -> Result<Integrity, TarballError> {
    let incomplete = |reason: &str| TarballError::IncompleteTar {
        url: String::new(),
        reason: reason.to_string(),
    };
    let trailer = gz_data
        .len()
        .checked_sub(8)
        .and_then(|start| gz_data.get(start..))
        .ok_or_else(|| incomplete("the gzip stream has no trailer"))?;
    let (crc, isize) = trailer.split_at(4);
    let mut computed = flate2::Crc::new();
    computed.update(tar_data);
    if crc != computed.sum().to_le_bytes() || isize != computed.amount().to_le_bytes() {
        return Err(incomplete("the gzip trailer does not describe the decoded tar"));
    }
    let mut opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    opts.input(tar_data);
    Ok(opts.result())
}
