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
    capabilities::{FsIsExecutable, FsReadFile},
    collation::en_collator,
    manifest_entry::is_manifest_entry,
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
/// absolute source) into `writer`, with `injected` packed in as well, in
/// the order [`compression_ordered_entries`] returns. Manifest entries
/// carry `manifest_json` instead of their on-disk bytes and are written
/// under [`PACKED_MANIFEST_NAME`]; entries whose source path is in `bins`
/// or whose source file on disk is executable are marked executable.
pub fn build_tarball<Sys: FsReadFile + FsIsExecutable>(
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
    let bin_set: HashSet<&Path> = bins
        .iter()
        .map(PathBuf::as_path)
        .collect();

    let mut builder = tar::Builder::new(GzEncoder::new(writer, compression));
    for entry in compression_ordered_entries(files_map, injected) {
        let file_data;
        let (data, mode) = match entry.source {
            EntrySource::Manifest(path) => (manifest_json, bin_mode::<Sys>(&bin_set, path)?),
            EntrySource::Injected(data) => (data, REGULAR_MODE),
            EntrySource::File(path) => {
                file_data = Sys::read_file(path)?;
                (file_data.as_slice(), bin_mode::<Sys>(&bin_set, path)?)
            }
        };
        append_entry(&mut builder, entry.name, data, mode)?;
    }

    builder.into_inner()?.finish()?;
    Ok(())
}

/// Every entry the archive will carry, in npm-packlist's compression
/// order: lowercased extension, then lowercased basename, then the path,
/// each compared with [`en_collator`]. The fixed order also keeps a re-pack
/// of unchanged sources byte-identical.
fn compression_ordered_entries<'a>(
    files_map: &'a IndexMap<String, PathBuf>,
    injected: &'a [(String, Vec<u8>)],
) -> Vec<QueuedEntry<'a>> {
    let mut entries: Vec<QueuedEntry<'a>> = files_map
        .iter()
        .map(|(name, source)| {
            if is_manifest_entry(name) {
                queued_entry(PACKED_MANIFEST_NAME, EntrySource::Manifest(source.as_path()))
            } else {
                queued_entry(name, EntrySource::File(source.as_path()))
            }
        })
        .chain(
            injected
                .iter()
                .map(|(name, data)| queued_entry(name, EntrySource::Injected(data))),
        )
        .collect();
    // Grouping by extension and basename keeps the same file name from
    // every template directory adjacent, so DEFLATE's window matches the
    // repeated content instead of storing each copy in full.
    let collator = en_collator();
    entries.sort_by(|left, right| {
        collator
            .compare(&left.ext, &right.ext)
            .then_with(|| collator.compare(&left.base, &right.base))
            .then_with(|| collator.compare(left.name, right.name))
    });
    entries
}

/// Where a tar entry's bytes come from at write time: file contents are
/// read only once the sorted write order is known, so the archive stays
/// streamed rather than buffered. The manifest carries its source path so
/// an `executableFiles` entry naming it still marks it executable.
enum EntrySource<'a> {
    Manifest(&'a Path),
    Injected(&'a [u8]),
    File(&'a Path),
}

/// One entry queued for the archive, decorated with its lowercased
/// extension and basename so the sort does not recompute them on every
/// comparison.
struct QueuedEntry<'a> {
    ext: String,
    base: String,
    name: &'a str,
    source: EntrySource<'a>,
}

fn queued_entry<'a>(name: &'a str, source: EntrySource<'a>) -> QueuedEntry<'a> {
    let base = basename(name);
    QueuedEntry { ext: extname(base).to_lowercase(), base: base.to_lowercase(), name, source }
}

/// The name's last `/`-separated component. Tar entry names are always
/// `/`-separated, so this stays a plain string split rather than going
/// through [`Path`], whose component split follows the host platform.
fn basename(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// The extension Node's `path.extname` reports, leading `.` included, so
/// the key matches the one pnpm 11 sorts on. [`Path::extension`] drops the
/// dot and reports `a.` and `a` alike, which would group a name ending in a
/// bare dot with the extensionless names instead of ahead of them.
pub(super) fn extname(base: &str) -> &str {
    match base.rfind('.') {
        // A dot only at the front names a dotfile, not an extension.
        None | Some(0) => "",
        Some(dot) => &base[dot..],
    }
}

fn bin_mode<Sys: FsIsExecutable>(bin_set: &HashSet<&Path>, source: &Path) -> io::Result<u32> {
    if bin_set.contains(source) || Sys::is_executable(source)? {
        Ok(EXECUTABLE_MODE)
    } else {
        Ok(REGULAR_MODE)
    }
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
