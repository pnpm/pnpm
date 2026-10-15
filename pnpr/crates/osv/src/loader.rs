use std::{fs::File, io::Read, path::Path};

use sha2::{Digest, Sha256};

use pnpr_error::RegistryError;

use super::{
    MAX_OSV_NAME_BYTES, MAX_OSV_RECORD_BYTES, OsvIndex, ingest_record_bytes, invalid_config,
};

pub(super) fn load_from_zip(path: &Path) -> Result<OsvIndex, RegistryError> {
    let file = File::open(path)
        .map_err(|err| {
            invalid_config(format!("failed to open OSV database {}: {err}", path.display()))
        })?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|err| {
            invalid_config(format!("failed to read OSV zip {}: {err}", path.display()))
        })?;
    let mut packages = std::collections::HashMap::new();
    let mut digests = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| {
                invalid_config(format!("failed to read OSV zip entry in {}: {err}", path.display()))
            })?;
        if !entry.is_file() || !entry.name().ends_with(".json") {
            continue;
        }
        let (name, bytes) = read_zip_record(&mut entry)?;
        digests.push(record_digest(&name, &bytes));
        ingest_record_bytes(&mut packages, &name, &bytes)?;
    }
    Ok(OsvIndex { packages, fingerprint: combine_fingerprint(digests) })
}

pub(super) fn load_from_directory(path: &Path) -> Result<OsvIndex, RegistryError> {
    let entries = std::fs::read_dir(path)
        .map_err(|err| {
            invalid_config(format!("failed to read OSV directory {}: {err}", path.display()))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            invalid_config(format!("failed to read OSV directory {}: {err}", path.display()))
        })?;

    let mut packages = std::collections::HashMap::new();
    let mut digests = Vec::new();
    for entry in entries {
        let entry_path = entry.path();
        if !entry_path.is_file()
            || entry_path
                .extension()
                .is_none_or(|extension| extension != "json")
        {
            continue;
        }
        let bytes = read_directory_record(&entry_path)?;
        let name = entry_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        digests.push(record_digest(name, &bytes));
        ingest_record_bytes(&mut packages, name, &bytes)?;
    }
    Ok(OsvIndex { packages, fingerprint: combine_fingerprint(digests) })
}

fn read_zip_record(
    entry: &mut zip::read::ZipFile<'_, File>,
) -> Result<(String, Vec<u8>), RegistryError> {
    if entry.name().len() > MAX_OSV_NAME_BYTES {
        return Err(invalid_config(format!(
            "OSV zip entry name is {} bytes, over the {MAX_OSV_NAME_BYTES}-byte limit",
            entry.name().len(),
        )));
    }
    let name = entry.name().to_string();
    if entry.size() > MAX_OSV_RECORD_BYTES {
        return Err(invalid_config(format!(
            "OSV zip entry {name} is {} bytes, over the {MAX_OSV_RECORD_BYTES}-byte per-record limit",
            entry.size(),
        )));
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    (&mut *entry)
        .take(MAX_OSV_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| invalid_config(format!("failed to read OSV zip entry {name}: {err}")))?;
    if bytes.len() as u64 > MAX_OSV_RECORD_BYTES {
        return Err(invalid_config(format!(
            "OSV zip entry {name} is over the {MAX_OSV_RECORD_BYTES}-byte per-record limit",
        )));
    }
    Ok((name, bytes))
}

fn read_directory_record(entry_path: &Path) -> Result<Vec<u8>, RegistryError> {
    let file = open_osv_record(entry_path)
        .map_err(|err| {
            invalid_config(format!("failed to read OSV record {}: {err}", entry_path.display()))
        })?;
    let is_regular_file = file.metadata().is_ok_and(|metadata| metadata.is_file());
    if !is_regular_file {
        return Err(invalid_config(format!(
            "OSV record {} is not a regular file",
            entry_path.display(),
        )));
    }
    let mut bytes = Vec::new();
    file.take(MAX_OSV_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| {
            invalid_config(format!("failed to read OSV record {}: {err}", entry_path.display()))
        })?;
    if bytes.len() as u64 > MAX_OSV_RECORD_BYTES {
        return Err(invalid_config(format!(
            "OSV record {} is over the {MAX_OSV_RECORD_BYTES}-byte per-record limit",
            entry_path.display(),
        )));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn open_osv_record(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_osv_record(path: &Path) -> std::io::Result<File> {
    File::open(path)
}

fn record_digest(name: &str, bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update((name.len() as u64).to_le_bytes());
    hasher.update(name.as_bytes());
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
    hasher.finalize().into()
}

fn combine_fingerprint(mut digests: Vec<[u8; 32]>) -> String {
    digests.sort_unstable();
    let mut hasher = Sha256::new();
    for digest in &digests {
        hasher.update(digest);
    }
    format!("sha256:{:x}", hasher.finalize())
}
