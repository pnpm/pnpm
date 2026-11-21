//! Bounded reads of distribution metadata without extracting files.

#[cfg(test)]
mod tests;

use crate::DistributionKind;
use std::io::{
    Cursor,
    Read,
};

/// Maximum size of a distribution metadata document.
pub const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;
const ARCHIVE_LIMIT: u64 = 512 * 1024 * 1024;

/// Read the sole top-level distribution metadata file from a ZIP archive.
pub fn metadata_from_zip(
    bytes: &[u8],
    filename: &str,
    kind: DistributionKind,
) -> Result<Vec<u8>, String> {
    metadata_from_zip_with_limit(bytes, filename, kind, ARCHIVE_LIMIT)
}

fn metadata_from_zip_with_limit(
    bytes: &[u8],
    filename: &str,
    kind: DistributionKind,
    limit: u64,
) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|err| format!("read archive {filename}: {err}"))?;
    let mut remaining = limit;
    archive.validate_header_consistency().map_err(|err| err.to_string())?;
    let mut paths = pnpr_package_name::ArchivePaths::default();
    let mut document = None;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|err| err.to_string())?;
        let size = entry.size();
        remaining = remaining
            .checked_sub(size)
            .ok_or_else(|| format!("{filename} exceeds {limit} decompressed bytes"))?;
        inspect_zip_entry(entry, &mut paths, filename, kind, &mut document)?;
    }
    document.ok_or_else(|| {
        format!(
            "{filename} has no {}",
            match kind {
                DistributionKind::Wheel => "dist-info METADATA",
                DistributionKind::Sdist => "PKG-INFO",
            },
        )
    })
}

fn inspect_zip_entry(
    entry: zip::read::ZipFile<'_, Cursor<&[u8]>>,
    paths: &mut pnpr_package_name::ArchivePaths,
    filename: &str,
    kind: DistributionKind,
    document: &mut Option<Vec<u8>>,
) -> Result<(), String> {
    let size = entry.size();
    let name = entry.name().map_err(|err| err.to_string())?;
    let path = paths
        .insert(&name, entry.is_dir())
        .ok_or_else(|| format!("invalid or duplicate archive path {name:?}"))?;
    if entry.is_symlink() {
        return Err(format!("unsupported archive entry {name:?}"));
    }
    if !is_metadata(path, kind) {
        let actual = std::io::copy(&mut entry.take(size + 1), &mut std::io::sink())
            .map_err(|err| err.to_string())?;
        if actual != size {
            return Err(format!("incorrect uncompressed size in {filename}"));
        }
        return Ok(());
    }
    if document.is_some() || !entry.is_file() {
        return Err(format!("multiple metadata files in {filename}"));
    }
    verify_metadata_directory(&name, filename, kind)?;
    let metadata = read_metadata(entry.take(size + 1))?;
    if metadata.len() as u64 != size {
        return Err(format!("incorrect metadata size in {filename}"));
    }
    *document = Some(metadata);
    Ok(())
}

fn is_metadata(path: &str, kind: DistributionKind) -> bool {
    let mut segments = path.split('/');
    let Some(directory) = segments.next() else { return false };
    let (suffix, filename) = match kind {
        DistributionKind::Wheel => (".dist-info", "METADATA"),
        DistributionKind::Sdist => ("", "PKG-INFO"),
    };
    !directory.is_empty()
        && directory.ends_with(suffix)
        && segments.next() == Some(filename)
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
pub fn metadata_from_sdist(bytes: &[u8], filename: &str) -> Result<Vec<u8>, String> {
    let mut paths = pnpr_package_name::ArchivePaths::default();
    let reader = flate2::read::MultiGzDecoder::new(bytes).take(ARCHIVE_LIMIT + 1);
    let mut archive = tar::Archive::new(reader);
    let mut document = None;
    for entry in archive.entries().map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        let raw = entry.path_bytes();
        let name = std::str::from_utf8(&raw).map_err(|err| err.to_string())?;
        let entry_type = entry.header().entry_type();
        let path = paths
            .insert(name, entry_type.is_dir())
            .ok_or_else(|| format!("invalid or duplicate archive path {name:?}"))?;
        if !entry_type.is_dir() && !entry_type.is_file() {
            return Err(format!("unsupported archive entry {name:?}"));
        }
        if !is_metadata(path, DistributionKind::Sdist) {
            continue;
        }
        if document.is_some() || !entry_type.is_file() {
            return Err("sdist must carry one regular root PKG-INFO".to_string());
        }
        verify_metadata_directory(path, filename, DistributionKind::Sdist)?;
        document = Some(read_metadata(entry)?);
    }
    let mut reader = archive.into_inner();
    std::io::copy(&mut reader, &mut std::io::sink()).map_err(|err| err.to_string())?;
    if reader.limit() == 0 {
        return Err(format!("sdist exceeds {ARCHIVE_LIMIT} decompressed bytes"));
    }
    document.ok_or_else(|| "sdist has no root PKG-INFO".to_string())
}

fn verify_metadata_directory(
    path: &str,
    filename: &str,
    kind: DistributionKind,
) -> Result<(), String> {
    let distribution = crate::parse_distribution_filename(filename).map_err(|err| err.to_string())?;
    let directory = path
        .split('/')
        .next()
        .unwrap_or_default();
    let directory = match kind {
        DistributionKind::Wheel => directory.trim_end_matches(".dist-info"),
        DistributionKind::Sdist => directory,
    };
    let (name, version) = directory
        .rsplit_once('-')
        .ok_or_else(|| format!("invalid distribution directory in {filename}"))?;
    if crate::normalize_name(name).map_err(|err| err.to_string())? != distribution.name
        || version.parse::<pep440_rs::Version>().map_err(|err| err.to_string())?
            != distribution.version.parse::<pep440_rs::Version>().map_err(|err| err.to_string())?
    {
        return Err(format!("distribution directory does not match {filename}"));
    }
    Ok(())
}
