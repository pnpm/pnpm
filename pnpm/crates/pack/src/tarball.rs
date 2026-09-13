//! Builds the gzipped tar archive a `pacquet pack` writes to disk.
//!
//! Every entry is stamped with a fixed mtime so the archive is
//! reproducible, executable bins get mode `0o755` and everything else
//! `0o644`, and any `package.json` / `package.json5` / `package.yaml`
//! entry is replaced by the serialized publish manifest under the name
//! `package/package.json`.
//!
//! The archive is streamed into the caller-provided `writer` (the
//! [`FsAtomicWrite`](crate::capabilities::FsAtomicWrite) temp file) rather
//! than buffered in memory, so peak memory does not scale with the total
//! packed size. Each file's bytes are still read fully through
//! [`FsReadFile`] (bounded by the largest single file), one `readFileSync`
//! per entry.

use crate::{
    capabilities::FsReadFile, contents::compare_paths_en_locale, manifest_entry::is_manifest_entry,
};
use flate2::{Compression, write::GzEncoder};
use indexmap::IndexMap;
use std::{
    collections::HashSet,
    io::{self, Write},
    path::{Path, PathBuf},
};

/// Fixed modification time stamped on every tar entry: 1985-10-26
/// 08:15:00 UTC, the "Back to the Future" timestamp npm uses so a
/// re-pack of unchanged sources produces a byte-identical archive.
/// This is `new Date('1985-10-26T08:15:00.000Z')` expressed in seconds.
const REPRODUCIBLE_MTIME: u64 = 499_162_500;

const EXECUTABLE_MODE: u32 = 0o755;
const REGULAR_MODE: u32 = 0o644;

/// The name every manifest entry is rewritten to inside the archive.
const PACKED_MANIFEST_NAME: &str = "package/package.json";

/// Stream the gzipped tar archive for `files_map` (`package/<path>` →
/// absolute source) into `writer`, with `injected` packed in as well.
/// Entries are written in npm-packlist's compression order — extension,
/// then basename, then full path — regardless of the maps' insertion
/// order. Files sharing a name tend to share contents across generated
/// trees, and that adjacency keeps them inside DEFLATE's window so the
/// duplicates dedupe instead of repeating; a full-path order scatters
/// them and has packed some packages at nine times the size
/// (pnpm/pnpm#14766). The fixed order also keeps a re-pack
/// byte-identical. Manifest entries carry `manifest_json` instead of
/// their on-disk bytes and are written under [`PACKED_MANIFEST_NAME`];
/// entries whose source path is in `bins` are marked executable.
pub fn build_tarball<Sys: FsReadFile>(
    writer: &mut dyn Write,
    files_map: &IndexMap<String, PathBuf>,
    manifest_json: &[u8],
    bins: &[PathBuf],
    gzip_level: Option<u32>,
    injected: &[(String, Vec<u8>)],
) -> io::Result<()> {
    let compression =
        gzip_level.map_or_else(Compression::default, |level| Compression::new(level.min(9)));
    // Hash the executable sources once instead of scanning `bins` for each
    // file (`publishConfig.executableFiles` can make both lists large).
    let bin_set: HashSet<&Path> = bins.iter().map(PathBuf::as_path).collect();

    let mut entries: Vec<QueuedEntry<'_>> = files_map
        .iter()
        .map(|(name, source)| {
            if is_manifest_entry(name) {
                queued_entry(PACKED_MANIFEST_NAME.to_string(), EntrySource::Manifest)
            } else {
                queued_entry(name.clone(), EntrySource::File(source.as_path()))
            }
        })
        .chain(
            injected
                .iter()
                .map(|(name, data)| queued_entry(name.clone(), EntrySource::Injected(data))),
        )
        .collect();
    entries.sort_by(|left, right| {
        left.ext
            .cmp(&right.ext)
            .then_with(|| left.base.cmp(&right.base))
            .then_with(|| compare_paths_en_locale(&left.name, &right.name))
    });

    let mut builder = tar::Builder::new(GzEncoder::new(writer, compression));
    write_entries::<Sys>(&mut builder, &entries, manifest_json, &bin_set)?;

    builder.into_inner()?.finish()?;
    Ok(())
}

/// Write the queued entries in their sorted order, reading each file's
/// bytes only when its turn comes so the archive stays streamed.
fn write_entries<Sys: FsReadFile>(
    builder: &mut tar::Builder<GzEncoder<&mut dyn Write>>,
    entries: &[QueuedEntry<'_>],
    manifest_json: &[u8],
    bin_set: &HashSet<&Path>,
) -> io::Result<()> {
    for entry in entries {
        let file_data;
        let (data, mode) = match &entry.source {
            EntrySource::Manifest => (manifest_json, REGULAR_MODE),
            EntrySource::Injected(data) => (*data, REGULAR_MODE),
            EntrySource::File(path) => {
                file_data = Sys::read_file(path)?;
                (file_data.as_slice(), bin_mode(bin_set, path))
            }
        };
        append_entry(builder, &entry.name, data, mode)?;
    }
    Ok(())
}

/// Where a tar entry's bytes come from at write time. File contents are
/// read only once the sorted write order is known, keeping the archive
/// streamed rather than buffered.
enum EntrySource<'a> {
    Manifest,
    Injected(&'a [u8]),
    File(&'a Path),
}

/// One entry queued for the archive, decorated with the compression-order
/// sort keys (lowercased extension and basename) so the sort does not
/// recompute them on every comparison.
struct QueuedEntry<'a> {
    ext: String,
    base: String,
    name: String,
    source: EntrySource<'a>,
}

fn queued_entry(name: String, source: EntrySource<'_>) -> QueuedEntry<'_> {
    let path = Path::new(&name);
    QueuedEntry {
        ext: path.extension().map_or_else(String::new, |ext| ext.to_string_lossy().to_lowercase()),
        base: path
            .file_name()
            .map_or_else(String::new, |base| base.to_string_lossy().to_lowercase()),
        name,
        source,
    }
}

fn bin_mode(bin_set: &HashSet<&Path>, source: &Path) -> u32 {
    if bin_set.contains(source) { EXECUTABLE_MODE } else { REGULAR_MODE }
}

fn append_entry<Writer: Write>(
    builder: &mut tar::Builder<Writer>,
    entry_name: &str,
    data: &[u8],
    mode: u32,
) -> io::Result<()> {
    // The POSIX ustar form npm writes: `ustar\0` magic and an explicit `0`
    // typeflag. Hand-rolled tar readers (e.g. publint) reject the GNU
    // defaults, mistaking a NUL typeflag for the end-of-archive marker.
    let mut header = tar::Header::new_ustar();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(data.len() as u64);
    header.set_mode(mode);
    header.set_mtime(REPRODUCIBLE_MTIME);
    // `append_data` sets the entry path and the header checksum.
    builder.append_data(&mut header, entry_name, data)
}
