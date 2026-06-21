//! Builds the gzipped tar archive a `pacquet pack` writes to disk.
//!
//! Mirrors upstream's `packPkg` at
//! [`pack.ts:355-388`](https://github.com/pnpm/pnpm/blob/cab1c11c69/releasing/commands/src/publish/pack.ts#L355-L388):
//! every entry is stamped with a fixed mtime so the archive is
//! reproducible, executable bins get mode `0o755` and everything else
//! `0o644`, and any `package.json` / `package.json5` / `package.yaml`
//! entry is replaced by the serialized publish manifest under the name
//! `package/package.json`.
//!
//! The archive is built in memory and returned as bytes; the caller
//! persists it through the [`FsWrite`](crate::capabilities::FsWrite)
//! seam. Source bytes are read through [`FsReadFile`].

use crate::{capabilities::FsReadFile, manifest_entry::is_manifest_entry};
use flate2::{Compression, write::GzEncoder};
use indexmap::IndexMap;
use std::{io, path::PathBuf};

/// Fixed modification time stamped on every tar entry: 1985-10-26
/// 08:15:00 UTC, the "Back to the Future" timestamp npm uses so a
/// re-pack of unchanged sources produces a byte-identical archive.
/// Matches upstream's `new Date('1985-10-26T08:15:00.000Z')` at
/// [`pack.ts:369`](https://github.com/pnpm/pnpm/blob/cab1c11c69/releasing/commands/src/publish/pack.ts#L369).
const REPRODUCIBLE_MTIME: u64 = 499_162_500;

const EXECUTABLE_MODE: u32 = 0o755;
const REGULAR_MODE: u32 = 0o644;

/// The name every manifest entry is rewritten to inside the archive.
const PACKED_MANIFEST_NAME: &str = "package/package.json";

/// Build the gzipped tar archive for `files_map` (ordered
/// `package/<path>` → absolute source). Manifest entries carry
/// `manifest_json` instead of their on-disk bytes; entries whose source
/// path is in `bins` are marked executable.
pub fn build_tarball<Sys: FsReadFile>(
    files_map: &IndexMap<String, PathBuf>,
    manifest_json: &[u8],
    bins: &[PathBuf],
    gzip_level: Option<u32>,
) -> io::Result<Vec<u8>> {
    let compression =
        gzip_level.map_or_else(Compression::default, |level| Compression::new(level.min(9)));
    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), compression));

    for (name, source) in files_map {
        let (entry_name, data) = if is_manifest_entry(name) {
            (PACKED_MANIFEST_NAME, manifest_json.to_vec())
        } else {
            (name.as_str(), Sys::read_file(source)?)
        };
        let mode =
            if bins.iter().any(|bin| bin == source) { EXECUTABLE_MODE } else { REGULAR_MODE };

        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(mode);
        header.set_mtime(REPRODUCIBLE_MTIME);
        // `append_data` sets the entry path and the header checksum.
        builder.append_data(&mut header, entry_name, data.as_slice())?;
    }

    builder.into_inner()?.finish()
}
