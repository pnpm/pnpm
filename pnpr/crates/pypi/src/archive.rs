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
    zip_metadata(bytes, filename, kind, Some(ARCHIVE_LIMIT))
}

/// Read only the metadata document when resolving a wheel from an upstream index.
pub fn metadata_from_wheel(bytes: &[u8], filename: &str) -> Result<Vec<u8>, String> {
    zip_metadata(bytes, filename, DistributionKind::Wheel, None)
}

fn zip_metadata(
    bytes: &[u8],
    filename: &str,
    kind: DistributionKind,
    limit: Option<u64>,
) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|err| format!("read archive {filename}: {err}"))?;
    let index = metadata_index(&archive, filename, kind)?;
    if let Some(limit) = limit {
        validate_zip(&mut archive, index, limit)?;
    }
    let entry = archive.by_index(index).map_err(|err| err.to_string())?;
    if !entry.is_file() {
        return Err(format!("{filename} must carry a regular metadata file"));
    }
    let size = entry.size();
    let bytes = read_metadata(entry)?;
    if bytes.len() as u64 != size {
        return Err(format!("incorrect metadata size in {filename}"));
    }
    Ok(bytes)
}

fn metadata_index(
    archive: &zip::ZipArchive<Cursor<&[u8]>>,
    filename: &str,
    kind: DistributionKind,
) -> Result<usize, String> {
    let mut found = None;
    for (index, name) in archive.file_names().enumerate() {
        let name = name.map_err(|err| err.to_string())?;
        if !is_metadata(&name, kind) {
            continue;
        }
        if found.replace(index).is_some() {
            return Err(format!("multiple metadata files in {filename}"));
        }
        verify_metadata_directory(&name, filename, kind)?;
    }
    found.ok_or_else(|| {
        let expected = match kind {
            DistributionKind::Wheel => "dist-info METADATA",
            DistributionKind::Sdist => "PKG-INFO",
        };
        format!("{filename} has no {expected}")
    })
}

fn validate_zip(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    metadata_index: usize,
    limit: u64,
) -> Result<(), String> {
    archive.validate_header_consistency().map_err(|err| err.to_string())?;
    let mut paths = pnpr_package_name::ArchivePaths::default();
    let mut remaining = limit;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|err| err.to_string())?;
        let name = entry.name().map_err(|err| err.to_string())?;
        remaining = remaining
            .checked_sub(entry.size())
            .ok_or_else(|| format!("ZIP exceeds {limit} decompressed bytes"))?;
        if entry.is_symlink() {
            return Err(format!("unsupported archive entry {name:?}"));
        }
        let path = if entry.is_dir() { paths.insert_directory(&name) } else { paths.insert(&name) };
        path.ok_or_else(|| format!("invalid or duplicate archive path {name:?}"))?;
        if index != metadata_index {
            verify_zip_size(entry)?;
        }
    }
    Ok(())
}

fn verify_zip_size(entry: zip::read::ZipFile<'_, Cursor<&[u8]>>) -> Result<(), String> {
    let size = entry.size();
    let actual = std::io::copy(&mut entry.take(size + 1), &mut std::io::sink())
        .map_err(|err| err.to_string())?;
    if actual != size {
        return Err("incorrect uncompressed ZIP entry size".to_string());
    }
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
        let path = sdist_entry_path(&mut paths, name, entry_type)?;
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

fn sdist_entry_path<'a>(
    paths: &mut pnpr_package_name::ArchivePaths,
    name: &'a str,
    entry_type: tar::EntryType,
) -> Result<&'a str, String> {
    let path = if entry_type.is_dir() {
        paths.insert_directory(name)
    } else if entry_type.is_file() {
        paths.insert(name)
    } else {
        return Err(format!("unsupported archive entry {name:?}"));
    };
    path.ok_or_else(|| format!("invalid or duplicate archive path {name:?}"))
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
