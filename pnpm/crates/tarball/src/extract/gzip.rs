use super::{
    BTreeMap, HashMap, IgnoreEntryFilter, MAX_UNTRUSTED_PREALLOC_BYTES, PackageBin,
    PackageFilesIndex, PathBuf, StoreDir, TarLimits, TarballError,
    extract_tarball_entries_with_bins, stream_extract_gzipped_tarball_with_tar,
};
use ssri::Integrity;
use tracing::instrument;
use zune_inflate::{DeflateDecoder, DeflateOptions, errors::DecodeErrorStatus};

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
    let trailer: [u8; 4] = gz_data
        .get(gz_data.len().checked_sub(4)?..)?
        .try_into()
        .ok()?;
    usize::try_from(u32::from_le_bytes(trailer)).ok()
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
/// streaming path ([`stream_extract_gzipped_tarball_with_tar`]) instead of the
/// eager whole-archive decompression
/// ([`decompress_gzip`] + [`extract_tarball_entries_with_bins`]).
///
/// The eager path materializes the entire decompressed archive as one
/// contiguous buffer, which for a large package multiplies across every
/// concurrently extracting task. Stream once either signal says the
/// archive is large: the exact compressed length, or an unpacked-size
/// claim ([`extract_gzipped_tarball_with_tar`] takes the larger of the
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
            let (files, index, bins) = extract_tarball_entries_with_bins(
                &tar_data,
                limits,
                store_dir,
                ignore_file_pattern,
            )?;
            Ok((files, index, bins, eager_tar_integrity(gz_data, &tar_data)))
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

/// The CAFS files, the store row, precalculated bins, and the outcome of hashing the decoded tar.
pub(crate) type GzipExtraction = (
    HashMap<String, PathBuf>,
    PackageFilesIndex,
    Option<BTreeMap<String, PackageBin>>,
    Result<Integrity, TarballError>,
);

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
