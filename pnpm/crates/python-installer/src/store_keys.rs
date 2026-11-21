//! The store index rows a `pylock.toml` keeps in use.

use pnpm_python_resolver::LockedWheel;
use pnpm_tarball::ArchiveStoreProjection;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    io,
    path::Path,
};

/// The package id a Python archive is stored under, named by its file.
pub(crate) fn archive_package_id(filename: &str) -> String {
    format!("python:{filename}")
}

/// The store index keys of the wheels and source distributions that the
/// `pylock.toml` at `path` pins, or `None` when there is no such file.
///
/// Only the file names and digests are read, so a lockfile another tool
/// wrote counts too. An archive without a SHA-256 digest was never stored
/// and adds no key.
///
/// # Errors
///
/// The file cannot be read, or does not parse as a `pylock.toml`.
pub fn locked_archive_store_index_keys(path: &Path) -> io::Result<Option<Vec<String>>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let lock: PinnedArchives =
        toml::from_str(&text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(Some(
        lock.packages
            .into_iter()
            .flat_map(|package| package.wheels.into_iter().chain(package.sdist))
            .filter_map(PinnedArchive::store_index_key)
            .collect(),
    ))
}

#[derive(Deserialize)]
struct PinnedArchives {
    #[serde(default)]
    packages: Vec<PinnedPackage>,
}

#[derive(Deserialize)]
struct PinnedPackage {
    #[serde(default)]
    wheels: Vec<PinnedArchive>,
    sdist: Option<PinnedArchive>,
}

#[derive(Deserialize)]
struct PinnedArchive {
    name: String,
    #[serde(default)]
    hashes: BTreeMap<String, String>,
}

impl PinnedArchive {
    fn store_index_key(self) -> Option<String> {
        let package_id = archive_package_id(&self.name);
        let integrity = LockedWheel { name: self.name, url: String::new(), hashes: self.hashes }
            .integrity()
            .ok()?;
        Some(ArchiveStoreProjection::RawArchive.store_index_key(
            &integrity.to_string(),
            &package_id,
        ))
    }
}

#[cfg(test)]
mod tests;
