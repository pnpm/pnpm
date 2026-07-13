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

use crate::{capabilities::FsReadFile, manifest_entry::is_manifest_entry};
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

/// Stream the gzipped tar archive for `files_map` (ordered
/// `package/<path>` → absolute source) into `writer`. Manifest entries
/// carry `manifest_json` instead of their on-disk bytes; entries whose
/// source path is in `bins` are marked executable.
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
    let mut builder = tar::Builder::new(GzEncoder::new(writer, compression));

    for (name, source) in files_map {
        let (entry_name, data) = if is_manifest_entry(name) {
            (PACKED_MANIFEST_NAME, manifest_json.to_vec())
        } else {
            (name.as_str(), Sys::read_file(source)?)
        };
        let mode = if bin_set.contains(source.as_path()) { EXECUTABLE_MODE } else { REGULAR_MODE };
        append_entry(&mut builder, entry_name, &data, mode)?;
    }
    for (name, data) in injected {
        append_entry(&mut builder, name, data, REGULAR_MODE)?;
    }

    builder.into_inner()?.finish()?;
    Ok(())
}

fn append_entry<W: Write>(
    builder: &mut tar::Builder<W>,
    entry_name: &str,
    data: &[u8],
    mode: u32,
) -> io::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(mode);
    header.set_mtime(REPRODUCIBLE_MTIME);
    // `append_data` sets the entry path and the header checksum.
    builder.append_data(&mut header, entry_name, data)
}
