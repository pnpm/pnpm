//! Bounded reads of distribution metadata without extracting files.

use std::io::{Cursor, Read};

/// Maximum size of a distribution metadata document.
pub const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;
const ARCHIVE_LIMIT: u64 = 512 * 1024 * 1024;

/// Read the sole top-level distribution metadata file from a ZIP archive.
pub fn metadata_from_zip(bytes: &[u8], filename: &str, wheel: bool) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|err| format!("read archive {filename}: {err}"))?;
    let mut document = None;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|err| err.to_string())?;
        let name = entry.name().map_err(|err| err.to_string())?;
        if is_metadata(&name, wheel) {
            if document.is_some() {
                return Err(format!("multiple metadata files in {filename}"));
            }
            if wheel {
                verify_wheel_directory(&name, filename)?;
            }
            document = Some(read_metadata(entry)?);
        }
    }
    document.ok_or_else(|| {
        format!("{filename} has no {}", if wheel { "dist-info METADATA" } else { "PKG-INFO" })
    })
}

fn is_metadata(path: &str, wheel: bool) -> bool {
    let mut segments = path.split('/');
    let Some(directory) = segments.next() else { return false };
    !directory.is_empty()
        && (!wheel || directory.ends_with(".dist-info"))
        && segments.next() == Some(if wheel { "METADATA" } else { "PKG-INFO" })
        && segments.next().is_none()
}

fn read_metadata(reader: impl Read) -> Result<Vec<u8>, String> {
    let mut document = Vec::new();
    reader
        .take(MAX_METADATA_BYTES as u64 + 1)
        .read_to_end(&mut document)
        .map_err(|err| err.to_string())?;
    if document.len() > MAX_METADATA_BYTES {
        return Err(format!("distribution metadata exceeds {MAX_METADATA_BYTES} bytes"));
    }
    Ok(document)
}

/// Read an sdist's root PKG-INFO from a gzip-compressed tar archive.
/// The decompressed stream is bounded, including entries skipped before it.
pub fn metadata_from_sdist(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let reader = flate2::read::GzDecoder::new(bytes).take(ARCHIVE_LIMIT + 1);
    let mut archive = tar::Archive::new(reader);
    let mut document = None;
    for entry in archive.entries().map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        if is_metadata(
            &entry
                .path()
                .map_err(|err| err.to_string())?
                .to_string_lossy(),
            false,
        ) {
            if document.is_some() || !entry.header().entry_type().is_file() {
                return Err("sdist must carry one regular root PKG-INFO".to_string());
            }
            document = Some(read_metadata(entry)?);
        }
    }
    let mut reader = archive.into_inner();
    std::io::copy(&mut reader, &mut std::io::sink()).map_err(|err| err.to_string())?;
    if reader.limit() == 0 {
        return Err(format!("sdist exceeds {ARCHIVE_LIMIT} decompressed bytes"));
    }
    document.ok_or_else(|| "sdist has no root PKG-INFO".to_string())
}

fn verify_wheel_directory(path: &str, filename: &str) -> Result<(), String> {
    let distribution = crate::parse_distribution_filename(filename).map_err(|err| err.to_string())?;
    let directory = path
        .split('/')
        .next()
        .unwrap_or_default()
        .trim_end_matches(".dist-info");
    let (name, version) = directory
        .rsplit_once('-')
        .ok_or_else(|| format!("invalid dist-info directory in {filename}"))?;
    if crate::normalize_name(name).map_err(|err| err.to_string())? != distribution.name
        || version.parse::<pep440_rs::Version>().map_err(|err| err.to_string())?
            != distribution.version.parse::<pep440_rs::Version>().map_err(|err| err.to_string())?
    {
        return Err(format!("dist-info directory does not match {filename}"));
    }
    Ok(())
}
